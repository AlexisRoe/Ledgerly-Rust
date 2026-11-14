//! Where the database is, read from `SPRING_DATASOURCE_URL`.

/// Host, port, database name and connection parameters of the Postgres database.
///
/// Java hands `SPRING_DATASOURCE_URL` to the PostgreSQL JDBC driver, so the value starts with
/// `jdbc:postgresql://`. A deployment written for Java keeps working unchanged (ADR 0013); the plain
/// `postgres://` and `postgresql://` forms are accepted as well. The credentials are separate settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseUrl {
    host: String,
    port: u16,
    database: String,
    parameters: Vec<(String, String)>,
}

/// Why a database URL was refused. The messages never repeat the URL: one written by hand may hold a password.
/// They name a parameter, but never its value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidDatabaseUrl {
    #[error("must start with jdbc:postgresql://, postgresql:// or postgres://")]
    UnknownScheme,
    #[error("must not contain a user name or password; set SPRING_DATASOURCE_USERNAME and SPRING_DATASOURCE_PASSWORD")]
    HasCredentials,
    #[error("must name a host")]
    MissingHost,
    #[error("must name exactly one host")]
    SeveralHosts,
    #[error("has a port that is not a number from 1 to 65535")]
    InvalidPort,
    #[error("must name a database, as in jdbc:postgresql://host:5432/database")]
    MissingDatabase,
    #[error("has the parameter {0}, which Ledgerly does not support; supported are ApplicationName and sslmode")]
    UnsupportedParameter(String),
    #[error("has an sslmode Ledgerly does not support; supported are disable, allow and prefer (no TLS yet)")]
    UnsupportedSslMode,
}

/// The connection parameters Ledgerly passes on. Any other would be ignored silently, which for a setting
/// such as `ssl=true` would mean an unencrypted connection the operator did not ask for. So they are refused.
const SUPPORTED_PARAMETERS: [&str; 2] = ["ApplicationName", "sslmode"];
const SUPPORTED_SSL_MODES: [&str; 3] = ["disable", "allow", "prefer"];

const DEFAULT_PORT: u16 = 5432;

impl DatabaseUrl {
    /// Parses `jdbc:postgresql://host[:port]/database[?key=value&…]` or the same without the `jdbc:` prefix.
    ///
    /// The port defaults to 5432. An IPv6 host is written in brackets: `[::1]`.
    ///
    /// # Errors
    ///
    /// Returns an [`InvalidDatabaseUrl`] that says what is wrong without quoting the URL.
    pub fn parse(url: &str) -> Result<Self, InvalidDatabaseUrl> {
        let url = url.trim();
        let rest = ["jdbc:postgresql://", "postgresql://", "postgres://"]
            .iter()
            .find_map(|scheme| url.strip_prefix(scheme))
            .ok_or(InvalidDatabaseUrl::UnknownScheme)?;

        let (location, query) = rest.split_once('?').unwrap_or((rest, ""));
        let (authority, database) = location.split_once('/').ok_or(InvalidDatabaseUrl::MissingDatabase)?;
        if authority.contains('@') {
            return Err(InvalidDatabaseUrl::HasCredentials);
        }
        if authority.contains(',') {
            return Err(InvalidDatabaseUrl::SeveralHosts);
        }
        if database.is_empty() || database.contains('/') {
            return Err(InvalidDatabaseUrl::MissingDatabase);
        }

        let (host, port) = if let Some(bracketed) = authority.strip_prefix('[') {
            let (host, after) = bracketed.split_once(']').ok_or(InvalidDatabaseUrl::MissingHost)?;
            match after.strip_prefix(':') {
                Some(port) => (host, Some(port)),
                None if after.is_empty() => (host, None),
                None => return Err(InvalidDatabaseUrl::InvalidPort),
            }
        } else {
            match authority.split_once(':') {
                Some((host, port)) => (host, Some(port)),
                None => (authority, None),
            }
        };
        if host.is_empty() {
            return Err(InvalidDatabaseUrl::MissingHost);
        }
        let port = match port {
            None => DEFAULT_PORT,
            Some(text) => match text.parse::<u16>() {
                Ok(port) if port != 0 => port,
                _ => return Err(InvalidDatabaseUrl::InvalidPort),
            },
        };

        let parameters = query
            .split('&')
            .filter(|pair| !pair.is_empty())
            .map(|pair| {
                let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
                (key.to_owned(), value.to_owned())
            })
            .collect::<Vec<_>>();
        if parameters
            .iter()
            .any(|(key, _)| key.eq_ignore_ascii_case("user") || key.eq_ignore_ascii_case("password"))
        {
            return Err(InvalidDatabaseUrl::HasCredentials);
        }
        if let Some((key, _)) = parameters
            .iter()
            .find(|(key, _)| !SUPPORTED_PARAMETERS.contains(&key.as_str()))
        {
            return Err(InvalidDatabaseUrl::UnsupportedParameter(key.clone()));
        }
        if parameters
            .iter()
            .any(|(key, value)| key == "sslmode" && !SUPPORTED_SSL_MODES.contains(&value.as_str()))
        {
            return Err(InvalidDatabaseUrl::UnsupportedSslMode);
        }

        Ok(Self {
            host: host.to_owned(),
            port,
            database: database.to_owned(),
            parameters,
        })
    }

    /// The host name or address, without brackets.
    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }

    /// The database name.
    #[must_use]
    pub fn database(&self) -> &str {
        &self.database
    }

    /// The connection parameters after `?`, in the order written and not decoded, such as `("sslmode", "require")`.
    #[must_use]
    pub fn parameters(&self) -> &[(String, String)] {
        &self.parameters
    }

    /// The value of a connection parameter; the last one wins if it is given twice.
    #[must_use]
    pub fn parameter(&self, key: &str) -> Option<&str> {
        self.parameters
            .iter()
            .rev()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_jdbc_form_java_uses() {
        let url = DatabaseUrl::parse("jdbc:postgresql://postgres:5432/ledgerly").unwrap();
        assert_eq!((url.host(), url.port(), url.database()), ("postgres", 5432, "ledgerly"));
        assert!(url.parameters().is_empty());
    }

    #[test]
    fn the_plain_forms() {
        for text in ["postgres://db.internal:6543/app", "postgresql://db.internal:6543/app"] {
            let url = DatabaseUrl::parse(text).unwrap();
            assert_eq!((url.host(), url.port(), url.database()), ("db.internal", 6543, "app"));
        }
    }

    #[test]
    fn the_port_defaults_to_5432() {
        assert_eq!(
            DatabaseUrl::parse("jdbc:postgresql://db/ledgerly").unwrap().port(),
            5432
        );
    }

    #[test]
    fn parameters_are_kept_in_order() {
        let url =
            DatabaseUrl::parse("jdbc:postgresql://db/ledgerly?sslmode=disable&ApplicationName=ledgerly&sslmode=prefer")
                .unwrap();
        assert_eq!(
            url.parameters(),
            [
                ("sslmode".to_owned(), "disable".to_owned()),
                ("ApplicationName".to_owned(), "ledgerly".to_owned()),
                ("sslmode".to_owned(), "prefer".to_owned()),
            ]
        );
        assert_eq!(url.parameter("sslmode"), Some("prefer"));
        assert_eq!(url.parameter("ApplicationName"), Some("ledgerly"));
        assert_eq!(url.parameter("currentSchema"), None);
    }

    #[test]
    fn parameters_that_would_be_ignored_are_refused() {
        assert_eq!(
            DatabaseUrl::parse("jdbc:postgresql://db/ledgerly?ssl=true"),
            Err(InvalidDatabaseUrl::UnsupportedParameter("ssl".to_owned()))
        );
        assert_eq!(
            DatabaseUrl::parse("jdbc:postgresql://db/ledgerly?sslmode=require"),
            Err(InvalidDatabaseUrl::UnsupportedSslMode)
        );
        let error = DatabaseUrl::parse("jdbc:postgresql://db/ledgerly?sslpassword=hunter2").unwrap_err();
        assert!(!error.to_string().contains("hunter2"));
    }

    #[test]
    fn ipv6_hosts_are_written_in_brackets() {
        let url = DatabaseUrl::parse("jdbc:postgresql://[::1]:5433/ledgerly").unwrap();
        assert_eq!((url.host(), url.port()), ("::1", 5433));
        assert_eq!(DatabaseUrl::parse("postgres://[::1]/ledgerly").unwrap().port(), 5432);
    }

    #[test]
    fn what_is_refused_and_why() {
        use InvalidDatabaseUrl::*;
        for (text, error) in [
            ("", UnknownScheme),
            ("jdbc:mysql://db/ledgerly", UnknownScheme),
            ("jdbc:postgresql:ledgerly", UnknownScheme),
            ("http://db/ledgerly", UnknownScheme),
            ("postgres://user:hunter2@db/ledgerly", HasCredentials),
            ("jdbc:postgresql://db/ledgerly?user=u&password=hunter2", HasCredentials),
            ("jdbc:postgresql:///ledgerly", MissingHost),
            ("jdbc:postgresql://:5432/ledgerly", MissingHost),
            ("jdbc:postgresql://a:5432,b:5432/ledgerly", SeveralHosts),
            ("jdbc:postgresql://db:0/ledgerly", InvalidPort),
            ("jdbc:postgresql://db:65536/ledgerly", InvalidPort),
            ("jdbc:postgresql://db:abc/ledgerly", InvalidPort),
            ("jdbc:postgresql://[::1]x/ledgerly", InvalidPort),
            ("jdbc:postgresql://db", MissingDatabase),
            ("jdbc:postgresql://db/", MissingDatabase),
            ("jdbc:postgresql://db/a/b", MissingDatabase),
        ] {
            assert_eq!(DatabaseUrl::parse(text), Err(error.clone()), "{text}");
        }
    }

    #[test]
    fn an_error_never_repeats_the_url() {
        let error = DatabaseUrl::parse("postgres://user:hunter2@db/ledgerly").unwrap_err();
        assert!(!error.to_string().contains("hunter2"));
    }
}
