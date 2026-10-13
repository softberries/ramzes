use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::{Args, Parser, Subcommand};
use serde::Deserialize;

const DEFAULT_LISTEN: &str = "127.0.0.1:7269";
const DEFAULT_DATA_DIR: &str = "./data";
const DEFAULT_LEASE_MS: i64 = 60_000;

#[derive(Parser)]
#[command(name = "ramzes", version, about = "Self-hosted notification router")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
    #[command(flatten)]
    pub options: Options,
}

#[derive(Subcommand)]
pub enum Command {
    /// Run the server (the default).
    Serve,
    /// Write a consistent copy of the SQLite database to a new file.
    Backup { file: PathBuf },
    /// Print the OpenAPI description of the admin API.
    Openapi,
    /// Write the configuration as YAML to a file, or to standard output. Credentials and keys are left out.
    Export { file: Option<PathBuf> },
    /// Replace the configuration with an exported one. Stored credentials and source keys are kept.
    Import { file: PathBuf },
}

/// Settings given as flags or environment variables. Each overrides the config file.
#[derive(Args, Default)]
pub struct Options {
    /// A TOML file with the same settings.
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,
    /// Address to listen on.
    #[arg(long, env = "RAMZES_LISTEN", global = true)]
    pub listen: Option<String>,
    /// Where the SQLite database and the generated secret key live.
    #[arg(long, env = "RAMZES_DATA_DIR", global = true)]
    pub data_dir: Option<PathBuf>,
    /// sqlite://path or postgres://user:pass@host/db. Defaults to SQLite in the data directory.
    #[arg(
        long,
        env = "RAMZES_DATABASE_URL",
        global = true,
        hide_env_values = true
    )]
    pub database_url: Option<String>,
    /// Base64 of 32 bytes used to encrypt stored credentials. Generated when absent.
    #[arg(long, env = "RAMZES_SECRET_KEY", global = true, hide_env_values = true)]
    pub secret_key: Option<String>,
    /// Address people reach this instance at, for links in messages.
    #[arg(long, env = "RAMZES_PUBLIC_URL", global = true)]
    pub public_url: Option<String>,
    /// Mark the session cookie Secure. Use when serving over HTTPS.
    #[arg(long, env = "RAMZES_SECURE_COOKIES", global = true)]
    pub secure_cookies: Option<bool>,
    /// Refuse to deliver to loopback, private and link-local addresses.
    #[arg(long, env = "RAMZES_BLOCK_PRIVATE_ADDRESSES", global = true)]
    pub block_private_addresses: Option<bool>,
    /// Ask for this token on /metrics, as `Authorization: Bearer <token>`.
    #[arg(long, env = "RAMZES_METRICS_TOKEN", global = true, hide_env_values = true)]
    pub metrics_token: Option<String>,
    /// Milliseconds a worker may hold a claimed item before another may take it.
    #[arg(long, env = "RAMZES_LEASE_MS", global = true, hide = true)]
    pub lease_ms: Option<i64>,
}

/// The same settings as read from the config file.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    listen: Option<String>,
    data_dir: Option<PathBuf>,
    database_url: Option<String>,
    secret_key: Option<String>,
    public_url: Option<String>,
    secure_cookies: Option<bool>,
    block_private_addresses: Option<bool>,
    metrics_token: Option<String>,
    lease_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub listen: SocketAddr,
    pub data_dir: PathBuf,
    pub database_url: String,
    pub secret_key: Option<String>,
    pub public_url: Option<String>,
    pub secure_cookies: bool,
    pub block_private_addresses: bool,
    pub metrics_token: Option<String>,
    pub lease_ms: i64,
}

impl FileConfig {
    pub fn read(path: &Path) -> anyhow::Result<FileConfig> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("could not read {}", path.display()))?;
        toml::from_str(&text)
            .with_context(|| format!("{} is not a valid config file", path.display()))
    }
}

impl Config {
    /// Flag or environment variable first, then the config file, then the default.
    pub fn resolve(options: Options, file: FileConfig) -> anyhow::Result<Config> {
        let listen = options
            .listen
            .or(file.listen)
            .unwrap_or_else(|| DEFAULT_LISTEN.to_owned());
        let listen = listen
            .parse()
            .with_context(|| format!("listen address {listen:?} is not host:port"))?;
        let data_dir = options
            .data_dir
            .or(file.data_dir)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_DATA_DIR));
        let database_url = options
            .database_url
            .or(file.database_url)
            .unwrap_or_else(|| format!("sqlite://{}", data_dir.join("ramzes.db").display()));
        let lease_ms = options
            .lease_ms
            .or(file.lease_ms)
            .unwrap_or(DEFAULT_LEASE_MS);
        anyhow::ensure!(lease_ms > 0, "lease_ms must be positive");
        Ok(Config {
            listen,
            database_url,
            secret_key: options.secret_key.or(file.secret_key),
            public_url: options
                .public_url
                .or(file.public_url)
                .map(|url| url.trim_end_matches('/').to_owned()),
            secure_cookies: options
                .secure_cookies
                .or(file.secure_cookies)
                .unwrap_or(false),
            block_private_addresses: options
                .block_private_addresses
                .or(file.block_private_addresses)
                .unwrap_or(false),
            metrics_token: options
                .metrics_token
                .or(file.metrics_token)
                .filter(|token| !token.is_empty()),
            lease_ms,
            data_dir,
        })
    }

    pub fn load(options: Options) -> anyhow::Result<Config> {
        let file = match &options.config {
            Some(path) => FileConfig::read(path)?,
            None => FileConfig::default(),
        };
        Config::resolve(options, file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(text: &str) -> FileConfig {
        toml::from_str(text).unwrap()
    }

    #[test]
    fn defaults_apply_when_nothing_is_set() {
        let config = Config::resolve(Options::default(), FileConfig::default()).unwrap();
        assert_eq!(config.listen, "127.0.0.1:7269".parse().unwrap());
        assert_eq!(config.data_dir, PathBuf::from("./data"));
        assert_eq!(config.database_url, "sqlite://./data/ramzes.db");
        assert_eq!(config.secret_key, None);
        assert_eq!(config.public_url, None);
        assert!(!config.secure_cookies);
        assert_eq!(config.lease_ms, 60_000);
    }

    #[test]
    fn the_file_overrides_defaults() {
        let from_file = file(
            "listen = \"0.0.0.0:9000\"\ndata_dir = \"/var/lib/ramzes\"\nsecure_cookies = true\n\
             public_url = \"https://ramzes.example.com/\"\n",
        );
        let config = Config::resolve(Options::default(), from_file).unwrap();
        assert_eq!(config.listen, "0.0.0.0:9000".parse().unwrap());
        assert_eq!(config.database_url, "sqlite:///var/lib/ramzes/ramzes.db");
        assert!(config.secure_cookies);
        assert_eq!(
            config.public_url.as_deref(),
            Some("https://ramzes.example.com")
        );
    }

    #[test]
    fn flags_override_the_file() {
        let from_file = file(
            "listen = \"0.0.0.0:9000\"\ndatabase_url = \"postgres://file/db\"\nsecure_cookies = true\n",
        );
        let options = Options {
            listen: Some("127.0.0.1:1234".into()),
            database_url: Some("postgres://flag/db".into()),
            secure_cookies: Some(false),
            ..Options::default()
        };
        let config = Config::resolve(options, from_file).unwrap();
        assert_eq!(config.listen, "127.0.0.1:1234".parse().unwrap());
        assert_eq!(config.database_url, "postgres://flag/db");
        assert!(!config.secure_cookies);
    }

    #[test]
    fn flags_are_parsed_from_the_command_line() {
        let cli = Cli::try_parse_from(["ramzes", "--listen", "127.0.0.1:1", "backup", "/tmp/x.db"])
            .unwrap();
        assert_eq!(cli.options.listen.as_deref(), Some("127.0.0.1:1"));
        assert!(matches!(cli.command, Some(Command::Backup { .. })));
        assert!(Cli::try_parse_from(["ramzes"]).unwrap().command.is_none());
    }

    #[test]
    fn bad_values_are_explained() {
        let bad_listen = Options {
            listen: Some("nonsense".into()),
            ..Options::default()
        };
        let error = Config::resolve(bad_listen, FileConfig::default())
            .unwrap_err()
            .to_string();
        assert!(error.contains("nonsense"), "{error}");
        assert!(
            toml::from_str::<FileConfig>("lisen = \"x\"").is_err(),
            "unknown keys must be rejected"
        );
        let bad_lease = Options {
            lease_ms: Some(0),
            ..Options::default()
        };
        assert!(Config::resolve(bad_lease, FileConfig::default()).is_err());
    }
}
