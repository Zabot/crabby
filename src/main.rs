use std::{env, path::PathBuf, time::Duration};

use anyhow::{Context, Result};
use octocrab::{params, Octocrab};
use serde::Deserialize;
use tokio::{select, time};

async fn get_blocked(
    octocrab: &Octocrab,
    repos: &[(&str, &str)],
    config: &Config,
) -> Result<Vec<String>> {
    let mut waiting_on_you = Vec::new();
    for (owner, repo) in repos {
        let mut page = octocrab
            .pulls(*owner, *repo)
            .list()
            .state(params::State::Open)
            .per_page(100)
            .send()
            .await
            .context("failed to list prs")?;

        for pull in page.take_items() {
            if let Some(reviewers) = pull.requested_reviewers {
                if reviewers
                    .iter()
                    .find(|reviewer| reviewer.login == config.username)
                    .is_none()
                {
                    continue;
                }
            }

            let number = pull.number;
            if let Some(title) = pull.title {
                waiting_on_you.push(format!("[{repo}] #{number} {title}"))
            }
        }
    }

    Ok(waiting_on_you)
}

#[derive(Deserialize)]
struct Config {
    username: String,
    repos: Vec<String>,
    token: String,
}

use std::fs;

#[tokio::main]
async fn main() -> Result<()> {
    // TODO Use XDG_CONFIG_DIRS
    let home = env::var("HOME").context("failed to get home dir")?;
    let home = PathBuf::from(home);
    let config_path = home.join(".config/crabby/config.toml");
    let config_string = fs::read_to_string(&config_path)
        .with_context(|| format!("failed to get config file: {}", config_path.display()))?;

    let config: Config = toml::from_str(&config_string).context("failed to parse config")?;

    let octocrab = octocrab::OctocrabBuilder::default()
        .personal_token(config.token.clone())
        .build()?;

    let repos: Vec<_> = config
        .repos
        .iter()
        .filter_map(|r| r.split_once('/'))
        .collect();

    libnotify::init("crabby").unwrap();
    let notification = libnotify::Notification::new("Pending PRs", None, None);
    notification.set_urgency(libnotify::Urgency::Low);

    let mut interval = time::interval(Duration::from_secs(10));

    loop {
        select! {
            _ = tokio::signal::ctrl_c() => {
                break;
            }
            _ = interval.tick() => {
                let blocked = get_blocked(&octocrab, &repos[..], &config).await.context("failed to get blocked")?;
                let body = format!("Review requested\n--------\n{}", blocked.join("\n"));
                notification.update("Pending PR reviews", Some(body.as_str()), None).expect("failed to update notification");
                notification.show().context("failed to show notification").expect("failed to show notification");
            }
        }
    }

    libnotify::uninit();

    Ok(())
}
