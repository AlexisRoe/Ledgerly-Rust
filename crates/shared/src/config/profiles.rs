/// The Spring profiles named in `SPRING_PROFILES_ACTIVE`, which keep their Java meaning (ADR 0013).
///
/// They are independent switches, as in Spring: `local,keycloak` and `prod,keycloak` are the usual pairs, and
/// no profile at all is valid too (Spring's "default" profile: nothing is switched on).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Profiles {
    /// Development: human-readable logs, Swagger UI, demo data, a throw-away signing key, and the object store
    /// and scanner where `compose.yaml` puts them.
    pub local: bool,
    /// Like `local` for the signing key, but without demo data.
    pub test: bool,
    /// Nothing has a default; the app refuses to start when a required setting is missing.
    pub prod: bool,
    /// Tokens come from Keycloak; the app issues none of its own.
    pub keycloak: bool,
}

impl Profiles {
    /// Reads a comma-separated list, as Spring does: names are trimmed, empty entries are skipped.
    ///
    /// A name Ledgerly does not know switches nothing on. Spring accepts any profile name, so this is not an
    /// error.
    #[must_use]
    pub fn parse(list: &str) -> Self {
        let mut profiles = Self::default();
        for name in list.split(',').map(str::trim) {
            match name {
                "local" => profiles.local = true,
                "test" => profiles.test = true,
                "prod" => profiles.prod = true,
                "keycloak" => profiles.keycloak = true,
                _ => {}
            }
        }
        profiles
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_profile_switches_nothing_on() {
        assert_eq!(Profiles::parse(""), Profiles::default());
        assert!(!Profiles::default().local && !Profiles::default().prod);
    }

    #[test]
    fn profiles_combine() {
        let profiles = Profiles::parse("prod,keycloak");
        assert!(profiles.prod && profiles.keycloak && !profiles.local && !profiles.test);
    }

    #[test]
    fn names_are_trimmed_and_empty_entries_skipped() {
        let profiles = Profiles::parse(" local , ,test,");
        assert!(profiles.local && profiles.test && !profiles.prod);
    }

    #[test]
    fn unknown_names_and_other_spellings_switch_nothing_on() {
        assert_eq!(Profiles::parse("staging,PROD,Local"), Profiles::default());
    }
}
