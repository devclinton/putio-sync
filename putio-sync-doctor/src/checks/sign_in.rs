//! Sign-in options (feature 019): every provider that's on can complete a sign-in, and
//! the redirect URL it was registered with is stable

use super::{count, Ctx};
use crate::report::{Finding, Item, Status};
use putio_sync_core::sign_in::SignInConfig;

/// A provider that's on but can't sign anyone in, with what it lacks
fn incomplete(config: &SignInConfig) -> Vec<Item> {
    config
        .enabled_providers()
        .into_iter()
        .filter_map(|p| {
            let c = config.provider(p);
            let missing: Vec<&str> = [
                (c.client_id.trim().is_empty(), "client ID"),
                (c.client_secret.trim().is_empty(), "client secret"),
                (c.allowed_users.is_empty(), "allowed users"),
            ]
            .into_iter()
            .filter_map(|(gone, what)| gone.then_some(what))
            .collect();
            (!missing.is_empty()).then(|| {
                Item::new(p.as_str(), p.title()).detail(format!("No {}", missing.join(", ")))
            })
        })
        .collect()
}

/// Whether `url` is plain http to somewhere other than this machine, which providers
/// refuse as a redirect URL
fn plain_http_elsewhere(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://") else {
        return false;
    };
    let host = rest.split('/').next().unwrap_or("");
    let host = host.rsplit_once(':').map_or(host, |(h, _)| h);
    !matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

pub(crate) fn check(config: &SignInConfig) -> Finding {
    let providers = config.enabled_providers();
    if providers.is_empty() {
        return Finding::ok(if config.password.is_set() {
            "Password sign-in is on; no providers are."
        } else {
            "Only the API token signs in."
        });
    }
    let broken = incomplete(config);
    if !broken.is_empty() {
        return Finding::new(
            Status::Problem,
            format!("{} can't sign anyone in.", count(broken.len(), "provider")),
        )
        .suggest("Fill in the missing fields in Settings › Sign-in, or turn the provider off.")
        .items(broken);
    }
    match config.base_url() {
        None => Finding::new(
            Status::Warning,
            "No public URL is set, so each redirect URL follows the address the browser used.",
        )
        .suggest(
            "Set the public URL in Settings › Sign-in to the address you registered with the provider.",
        ),
        Some(url) if plain_http_elsewhere(url) => Finding::new(
            Status::Warning,
            format!("The public URL {url} uses http://; Google and Discord only accept https:// redirect URLs off this machine."),
        )
        .suggest("Serve putio-sync over HTTPS (a reverse proxy or tailscale serve) and use that address."),
        Some(_) => Finding::ok(format!(
            "{} on.",
            count(providers.len(), "provider")
        )),
    }
}

pub(crate) async fn sign_in(ctx: &Ctx) -> Finding {
    check(&ctx.config.sign_in)
}

#[cfg(test)]
mod tests {
    use super::*;
    use putio_sync_core::sign_in::Provider;

    fn on(config: &mut SignInConfig, provider: Provider) {
        let p = config.provider_mut(provider);
        p.enabled = true;
        p.client_id = "id".into();
        p.client_secret = "secret".into();
        p.allowed_users = vec!["me@example.com".into()];
    }

    #[test]
    fn nothing_on_is_ok() {
        assert_eq!(check(&SignInConfig::default()).status, Status::Ok);
    }

    #[test]
    fn a_provider_missing_fields_is_a_problem() {
        let mut config = SignInConfig::default();
        on(&mut config, Provider::Github);
        config.github.client_secret.clear();
        config.github.allowed_users.clear();
        let finding = check(&config);
        assert_eq!(finding.status, Status::Problem);
        assert_eq!(
            finding.items[0].detail.as_deref(),
            Some("No client secret, allowed users")
        );
    }

    #[test]
    fn the_public_url_is_checked() {
        let mut config = SignInConfig::default();
        on(&mut config, Provider::Google);
        assert_eq!(check(&config).status, Status::Warning);
        config.public_url = Some("http://nas.local:15432".into());
        assert_eq!(check(&config).status, Status::Warning);
        config.public_url = Some("http://localhost:15432".into());
        assert_eq!(check(&config).status, Status::Ok);
        config.public_url = Some("https://sync.example.com".into());
        assert_eq!(check(&config).status, Status::Ok);
    }
}
