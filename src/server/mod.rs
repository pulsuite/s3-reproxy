pub mod remote;
use crate::db::ListObjectTokens;
use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use itertools::Itertools;
use mongodb::bson::doc;
use mongodb::bson::oid::ObjectId;
use s3s::dto::{
    Bucket, GetBucketLocationInput, GetBucketLocationOutput, HeadBucketInput, HeadBucketOutput,
    ListBucketsInput, ListBucketsOutput, ListObjectsV2Input, ListObjectsV2Output,
};
use s3s::{s3_error, S3Error, S3Request, S3Response, S3Result, S3};
use s3s_aws::conv::AwsConversion;
use tokio::sync::oneshot;
use tracing::{error, info, instrument, warn};

use crate::db::MongoDB;

use self::remote::{RemoteMessage, S3Remote};

pub struct S3Reproxy {
    pub bucket: String,
    pub remotes: Arc<Vec<S3Remote>>,
    pub db: Arc<MongoDB>,
}

#[async_trait]
impl S3 for S3Reproxy {
    #[instrument(skip_all)]
    async fn list_buckets(
        &self,
        _req: S3Request<ListBucketsInput>,
    ) -> S3Result<S3Response<ListBucketsOutput>> {
        info!("(intercepted) {}", self.bucket);
        Ok(S3Response::new(ListBucketsOutput {
            buckets: Some(vec![Bucket {
                creation_date: None,
                name: Some(self.bucket.clone()),
            }]),
            owner: None,
        }))
    }

    #[instrument(skip_all, fields(bucket = req.input.bucket))]
    async fn get_bucket_location(
        &self,
        req: S3Request<GetBucketLocationInput>,
    ) -> S3Result<S3Response<GetBucketLocationOutput>> {
        if req.input.bucket != self.bucket {
            warn!("(intercepted) not found");
            return Err(s3_error!(NoSuchBucket));
        }

        let output = GetBucketLocationOutput::default();
        info!("(intercepted) ok");
        Ok(S3Response::new(output))
    }

    #[instrument(skip_all, fields(bucket = req.input.bucket))]
    async fn head_bucket(
        &self,
        req: S3Request<HeadBucketInput>,
    ) -> S3Result<S3Response<HeadBucketOutput>> {
        if req.input.bucket != self.bucket {
            warn!("(intercepted) not found");
            return Err(s3_error!(NoSuchBucket));
        }

        let output = HeadBucketOutput::default();
        info!("(intercepted) ok");
        Ok(S3Response::new(output))
    }

    #[instrument(skip_all, fields(token = &req.input.continuation_token), name = "s3s/list_objects_v2")]
    async fn list_objects_v2(
        &self,
        req: S3Request<ListObjectsV2Input>,
    ) -> S3Result<S3Response<ListObjectsV2Output>> {
        info!("{:?}", &req);

        let start_after = match req.input.continuation_token.clone() {
            Some(continuation_token) => {
                let list = self
                    .db
                    .list_object_tokens
                    .find_one_and_update(
                        doc! {
                            "_id": ObjectId::parse_str(continuation_token)
                                .map_err(|e| {
                                    warn!("(intercepted) invalid continuation token: {:?}", e);
                                    S3Error::new(s3s::S3ErrorCode::InvalidToken)
                                })?,
                        },
                        doc! {
                            "$set": {
                                "consumed_at": mongodb::bson::DateTime::now(),
                            },
                        },
                    )
                    .await
                    .map_err(|e| {
                        error!("mongodb error: {:?}", e);
                        S3Error::new(s3s::S3ErrorCode::InternalError)
                    })?
                    .ok_or_else(|| {
                        warn!("(intercepted) continuation token not found.");
                        S3Error::new(s3s::S3ErrorCode::InvalidToken)
                    })?;
                Some(list.start_after)
            }
            None => None,
        };

        let read_remotes = self.remotes.iter().sorted_by(|a, b| {
            b.read_request
                .cmp(&a.read_request)
                .then_with(|| a.priority.cmp(&b.priority))
        });

        let start_after = start_after.or(req.input.start_after.clone());

        let Some((result, remote)) = ('request: {
            for remote in read_remotes {
                let Some(output) = (try {
                    let (tx, rx) = oneshot::channel();
                    remote
                        .tx
                        .send(remote::RemoteMessage::ListObjects {
                            prefix: req.input.prefix.clone(),
                            delimiter: req.input.delimiter.clone(),
                            max_keys: req.input.max_keys,
                            start_after: start_after.clone(),
                            reply: tx,
                        })
                        .await
                        .ok()?;
                    rx.await.ok()??
                }) else {
                    warn!("remote({:?}) request failed. skipping", remote.name);
                    continue;
                };
                break 'request Some((output, remote.name.clone()));
            }
            None
        }) else {
            warn!("no remotes available!");
            return Err(s3_error!(InternalError));
        };

        let mut output = ListObjectsV2Output::try_from_aws(result)?;

        output.continuation_token = req.input.continuation_token;
        output.next_continuation_token = match output.next_continuation_token {
            Some(_) => 'm: {
                let Some(last) = output
                    .contents
                    .as_ref()
                    .and_then(|e| e.last())
                    .and_then(|e| e.key.clone())
                else {
                    break 'm None;
                };

                let list = self
                    .db
                    .list_object_tokens
                    .insert_one(ListObjectTokens {
                        start_after: last,
                        created_at: mongodb::bson::DateTime::now(),
                        consumed_at: None,
                    })
                    .await
                    .map_err(|e| {
                        error!("mongodb error: {:?}", e);
                        S3Error::new(s3s::S3ErrorCode::InternalError)
                    })?;

                Some(list.inserted_id.to_string())
            }
            None => None,
        };

        info!("ok (remote: {})", remote);
        Ok(S3Response::new(output))
    }
}
