//! Configuration: the only place the process environment is read.

use std::fmt;
use std::net::IpAddr;
use std::time::Duration;

use thiserror::Error;
use tracing_subscriber::EnvFilter;

const DEFAULT_HOST: &str = "0.0.0.0";
const DEFAULT_PORT: u32 = 8080;
const DEFAULT_MAX_CONNECTIONS: u32 = 10;
const DEFAULT_ACQUIRE_TIMEOUT_SECS: u32 = 5;
const DEFAULT_LOG_LEVEL: &str = "info";

/// A configuration problem. `Display` always starts with the variable name.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{variable} {problem}")]
pub struct ConfigError {
    pub variable: &'static str,
    pub problem: String,
}

impl ConfigError {
    fn new(variable: &'static str, problem: impl Into<String>) -> Self {
        Self {
            variable,
            problem: problem.into(),
        }
    }
}

/// Log output rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogFormat {
    /// One JSON object per line.
    #[default]
    Json,
    /// Human-readable single lines.
    Pretty,
}

/// Logging settings. `level` is an `EnvFilter` directive, already validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogConfig {
    pub format: LogFormat,
    pub level: String,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            format: LogFormat::Json,
            level: DEFAULT_LOG_LEVEL.to_owned(),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Config {
    pub database_url: String,
    pub host: IpAddr,
    pub port: u16,
    pub database_max_connections: u32,
    pub database_acquire_timeout: Duration,
    pub enable_test_routes: bool,
    pub log: LogConfig,
}

// The connection string may embed a password, so it is never printed.
impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("database_url", &"<redacted>")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("database_max_connections", &self.database_max_connections)
            .field("database_acquire_timeout", &self.database_acquire_timeout)
            .field("enable_test_routes", &self.enable_test_routes)
            .field("log", &self.log)
            .finish()
    }
}

impl Config {
    /// Reads the process environment.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// Builds a configuration from an arbitrary variable lookup. An empty
    /// value is treated as unset.
    pub fn from_lookup<F>(lookup: F) -> Result<Self, ConfigError>
    where
        F: Fn(&str) -> Option<String>,
    {
        let get = |name: &str| lookup(name).filter(|value| !value.trim().is_empty());

        let database_url =
            get("DATABASE_URL").ok_or_else(|| ConfigError::new("DATABASE_URL", "is required"))?;

        let host = match get("HOST") {
            None => DEFAULT_HOST
                .parse()
                .map_err(|_| ConfigError::new("HOST", "default is not an IP address"))?,
            Some(raw) => raw.trim().parse::<IpAddr>().map_err(|_| {
                ConfigError::new("HOST", format!("must be an IP address (got \"{raw}\")"))
            })?,
        };

        let port = parse_in_range(get("PORT"), "PORT", DEFAULT_PORT, 1, 65_535)?;
        let database_max_connections = parse_in_range(
            get("DATABASE_MAX_CONNECTIONS"),
            "DATABASE_MAX_CONNECTIONS",
            DEFAULT_MAX_CONNECTIONS,
            1,
            100,
        )?;
        let acquire_secs = parse_in_range(
            get("DATABASE_ACQUIRE_TIMEOUT_SECS"),
            "DATABASE_ACQUIRE_TIMEOUT_SECS",
            DEFAULT_ACQUIRE_TIMEOUT_SECS,
            1,
            300,
        )?;

        let enable_test_routes = match get("ENABLE_TEST_ROUTES").as_deref().map(str::trim) {
            None | Some("false") => false,
            Some("true") => true,
            Some(other) => {
                return Err(ConfigError::new(
                    "ENABLE_TEST_ROUTES",
                    format!("must be \"true\" or \"false\" (got \"{other}\")"),
                ));
            }
        };

        let format = match get("LOG_FORMAT").as_deref().map(str::trim) {
            None | Some("json") => LogFormat::Json,
            Some("pretty") => LogFormat::Pretty,
            Some(other) => {
                return Err(ConfigError::new(
                    "LOG_FORMAT",
                    format!("must be \"json\" or \"pretty\" (got \"{other}\")"),
                ));
            }
        };
        let level = match get("LOG_LEVEL") {
            None => DEFAULT_LOG_LEVEL.to_owned(),
            Some(raw) => {
                let directive = raw.trim().to_owned();
                EnvFilter::try_new(&directive).map_err(|err| {
                    ConfigError::new(
                        "LOG_LEVEL",
                        format!(
                            "must be a valid log filter directive (got \"{directive}\"): {err}"
                        ),
                    )
                })?;
                directive
            }
        };

        Ok(Self {
            database_url,
            host,
            // The range check above guarantees the value fits in a u16.
            port: u16::try_from(port).map_err(|_| ConfigError::new("PORT", "is out of range"))?,
            database_max_connections,
            database_acquire_timeout: Duration::from_secs(u64::from(acquire_secs)),
            enable_test_routes,
            log: LogConfig { format, level },
        })
    }
}

fn parse_in_range(
    raw: Option<String>,
    variable: &'static str,
    default: u32,
    min: u32,
    max: u32,
) -> Result<u32, ConfigError> {
    let Some(raw) = raw else {
        return Ok(default);
    };
    let problem = || {
        ConfigError::new(
            variable,
            format!("must be an integer between {min} and {max} (got \"{raw}\")"),
        )
    };
    let value = raw.trim().parse::<u32>().map_err(|_| problem())?;
    if (min..=max).contains(&value) {
        Ok(value)
    } else {
        Err(problem())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn config_with(vars: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let map: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        Config::from_lookup(|name| map.get(name).cloned())
    }

    const URL: (&str, &str) = ("DATABASE_URL", "postgres://u:p@localhost/db");

    #[test]
    fn defaults_apply_when_only_database_url_is_set() {
        let cfg = config_with(&[URL]).unwrap();
        assert_eq!(cfg.host.to_string(), "0.0.0.0");
        assert_eq!(cfg.port, 8080);
        assert_eq!(cfg.database_max_connections, 10);
        assert_eq!(cfg.database_acquire_timeout, Duration::from_secs(5));
        assert!(!cfg.enable_test_routes);
    }

    #[test]
    fn missing_database_url_is_reported() {
        let err = config_with(&[]).unwrap_err();
        assert_eq!(err.to_string(), "DATABASE_URL is required");
    }

    #[test]
    fn empty_database_url_counts_as_missing() {
        let err = config_with(&[("DATABASE_URL", "  ")]).unwrap_err();
        assert_eq!(err.variable, "DATABASE_URL");
    }

    #[test]
    fn explicit_values_are_used() {
        let cfg = config_with(&[
            URL,
            ("HOST", "127.0.0.1"),
            ("PORT", "9000"),
            ("DATABASE_MAX_CONNECTIONS", "3"),
            ("DATABASE_ACQUIRE_TIMEOUT_SECS", "1"),
            ("ENABLE_TEST_ROUTES", "true"),
        ])
        .unwrap();
        assert_eq!(cfg.host.to_string(), "127.0.0.1");
        assert_eq!(cfg.port, 9000);
        assert_eq!(cfg.database_max_connections, 3);
        assert_eq!(cfg.database_acquire_timeout, Duration::from_secs(1));
        assert!(cfg.enable_test_routes);
    }

    #[test]
    fn ipv6_host_is_accepted() {
        assert!(config_with(&[URL, ("HOST", "::1")]).is_ok());
    }

    #[test]
    fn invalid_values_name_their_variable() {
        let cases: &[(&str, &str)] = &[
            ("PORT", "70000"),
            ("PORT", "0"),
            ("PORT", "-1"),
            ("PORT", "http"),
            ("HOST", "localhost"),
            ("DATABASE_MAX_CONNECTIONS", "0"),
            ("DATABASE_MAX_CONNECTIONS", "101"),
            ("DATABASE_ACQUIRE_TIMEOUT_SECS", "abc"),
            ("DATABASE_ACQUIRE_TIMEOUT_SECS", "0"),
            ("DATABASE_ACQUIRE_TIMEOUT_SECS", "301"),
            ("ENABLE_TEST_ROUTES", "maybe"),
            ("ENABLE_TEST_ROUTES", "TRUE"),
        ];
        for (name, value) in cases {
            let err = config_with(&[URL, (name, value)]).unwrap_err();
            assert_eq!(err.variable, *name, "value {value:?}");
            assert!(err.to_string().starts_with(name), "{err}");
        }
    }

    #[test]
    fn boundary_values_are_accepted() {
        assert!(config_with(&[URL, ("PORT", "1")]).is_ok());
        assert!(config_with(&[URL, ("PORT", "65535")]).is_ok());
        assert!(config_with(&[URL, ("DATABASE_MAX_CONNECTIONS", "100")]).is_ok());
        assert!(config_with(&[URL, ("DATABASE_ACQUIRE_TIMEOUT_SECS", "300")]).is_ok());
    }

    #[test]
    fn log_defaults() {
        let cfg = config_with(&[URL]).unwrap();
        assert_eq!(cfg.log.format, LogFormat::Json);
        assert_eq!(cfg.log.level, "info");
    }

    #[test]
    fn log_format_values() {
        let json = config_with(&[URL, ("LOG_FORMAT", "json")]).unwrap();
        assert_eq!(json.log.format, LogFormat::Json);
        let pretty = config_with(&[URL, ("LOG_FORMAT", "pretty")]).unwrap();
        assert_eq!(pretty.log.format, LogFormat::Pretty);
    }

    #[test]
    fn log_format_rejects_other_values() {
        for value in ["xml", "JSON", "text"] {
            let err = config_with(&[URL, ("LOG_FORMAT", value)]).unwrap_err();
            assert_eq!(err.variable, "LOG_FORMAT", "value {value:?}");
            assert!(err.to_string().starts_with("LOG_FORMAT must be"), "{err}");
        }
    }

    #[test]
    fn log_level_accepts_directives() {
        for value in ["warn", "debug", "info,sqlx=warn"] {
            let cfg = config_with(&[URL, ("LOG_LEVEL", value)]).unwrap();
            assert_eq!(cfg.log.level, value);
        }
    }

    #[test]
    fn log_level_rejects_invalid_directive() {
        let err = config_with(&[URL, ("LOG_LEVEL", "info,sqlx=loud")]).unwrap_err();
        assert_eq!(err.variable, "LOG_LEVEL");
        assert!(err.to_string().starts_with("LOG_LEVEL"), "{err}");
    }

    #[test]
    fn empty_log_variables_use_defaults() {
        let cfg = config_with(&[URL, ("LOG_FORMAT", ""), ("LOG_LEVEL", "  ")]).unwrap();
        assert_eq!(cfg.log, LogConfig::default());
    }

    #[test]
    fn debug_output_redacts_the_database_url() {
        let cfg = config_with(&[("DATABASE_URL", "postgres://u:s3cr3t@h/db")]).unwrap();
        let printed = format!("{cfg:?}");
        assert!(!printed.contains("s3cr3t"));
        assert!(printed.contains("<redacted>"));
    }
}
