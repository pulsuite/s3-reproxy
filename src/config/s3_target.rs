use derivative::Derivative;
use serde::{Deserialize, Serialize};

#[derive(Derivative, Clone, Serialize, Deserialize, PartialEq)]
#[derivative(Debug)]
pub struct Config {
    pub remotes: Vec<S3Target>,
    pub access_key: String,
    #[derivative(Debug = "ignore")]
    pub secret_key: String,
    pub bucket: String,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Derivative)]
#[derivative(Debug)]
pub struct S3Credential {
    pub endpoint: String,
    pub access_key: String,
    #[derivative(Debug = "ignore")]
    pub secret_key: String,
    pub bucket: String,
}

const fn default_priority() -> u32 {
    1
}

const fn default_read_request() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct S3Target {
    /// The name of the target
    pub name: String,

    /// Read priority of this target.
    /// Read Requests to s3-reproxy are issued in order of priority.
    #[serde(default = "default_priority")]
    pub priority: u32,

    /// Whether this target is allowed to read?
    /// For requests to search for or retrieve a file, if all targets with read_request true respond "does not exist", s3-reproxy will not search for the file any further and will respond "does not exist".
    /// However, if all targets with read_request true are down, the one with read_request false and highest priority will be used for reading.
    #[serde(default = "default_read_request")]
    pub read_request: bool,

    pub s3: S3Credential,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn parse_target_with_default() {
        let yaml = r#"
            name: cloudflare-r2
            s3:
              endpoint: http://localhost:8080
              access_key: abcabc
              secret_key: defdef
              bucket: test
        "#;

        let target: S3Target = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            target,
            S3Target {
                name: "cloudflare-r2".to_string(),
                priority: 1,
                read_request: true,
                s3: S3Credential {
                    endpoint: "http://localhost:8080".to_string(),
                    access_key: "abcabc".to_string(),
                    secret_key: "defdef".to_string(),
                    bucket: "test".to_string(),
                },
            }
        );
    }

    #[test]
    fn parse_config() {
        let yaml = r#"
            targets:
            - name: cloudflare-r2
              priority: 3
              read_request: false
              s3:
                endpoint: http://localhost:8080
                access_key: abcabc
                secret_key: defdef
                bucket: test1
            - name: local-minio
              priority: 5
              read_request: true
              s3:
                endpoint: http://localhost:8080
                access_key: abcabc
                secret_key: defdef
                bucket: test2
        "#;

        let config: Config = serde_yaml::from_str(yaml).unwrap();

        assert_eq!(
            config.remotes,
            vec![
                S3Target {
                    name: "cloudflare-r2".to_string(),
                    priority: 3,
                    read_request: false,
                    s3: S3Credential {
                        endpoint: "http://localhost:8080".to_string(),
                        access_key: "abcabc".to_string(),
                        secret_key: "defdef".to_string(),
                        bucket: "test1".to_string(),
                    },
                },
                S3Target {
                    name: "local-minio".to_string(),
                    priority: 5,
                    read_request: true,
                    s3: S3Credential {
                        endpoint: "http://localhost:8080".to_string(),
                        access_key: "abcabc".to_string(),
                        secret_key: "defdef".to_string(),
                        bucket: "test2".to_string(),
                    },
                },
            ]
        );
    }
}
