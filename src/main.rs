#![warn(clippy::pedantic)]

mod tmp_response;

use std::env;
use std::process;
use std::time::Duration;

use anyhow::Context;
use chrono::Local;
use chrono::NaiveDateTime;
use chrono::Utc;
use regex::Regex;
use serenity::all::CreateAttachment;
use serenity::all::CreateScheduledEvent;
use serenity::all::GuildId;
use serenity::all::Ready;
use serenity::all::ScheduledEventType;
use serenity::all::Timestamp;
use serenity::async_trait;
use serenity::prelude::*;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::FmtSubscriber;

const TMP_BASE_URL: &str = "https://truckersmp.com";
const EVENT_API_URL: &str = "https://api.truckersmp.com/v2/vtc/{id}/events";
const EVENT_ATTENDING_API_URL: &str = "https://api.truckersmp.com/v2/vtc/{id}/events/attending";
const MARKDOWN_IMAGE_REGEX: &str = r"!(\[[^\]]*\])?\([^)]*\)";

struct Handler {
    data: Vec<tmp_response::EventIndex>,
}

impl Handler {
    async fn process_events(
        &self,
        guild_id: &GuildId,
        ctx: &serenity::client::Context,
    ) -> anyhow::Result<()> {
        let events = guild_id
            .scheduled_events(ctx.http(), false)
            .await
            .context("failed to get events")?;
        let mut new_events = vec![];

        // Figure out what events are new
        'outer: for event in &self.data {
            let event_id = *event.id();
            for ev in &events {
                if let Some(desc) = &ev.description {
                    if desc.contains(&format!("### {event_id} ###")) {
                        tracing::debug!("Event ID {event_id} already found, skipping...");
                        continue 'outer;
                    }
                }
            }

            new_events.push(event.clone());
        }

        // Add new events
        // SAFETY: The regex is checked externally
        let re = Regex::new(MARKDOWN_IMAGE_REGEX).unwrap();
        for event in new_events {
            let start_time: NaiveDateTime =
                NaiveDateTime::parse_from_str(event.start_at(), "%Y-%m-%d %H:%M:%S")
                    .unwrap_or(Local::now().naive_local());

            if start_time.and_utc() <= Utc::now() {
                // Skip if in the past
                tracing::info!("Skipping event {} as it is in the past.", event.id());
                continue;
            }

            let end_time = start_time + Duration::from_secs(60 * 60);

            let desc_prefix = format!("[See on TruckersMP]({}{})\n\n", TMP_BASE_URL, event.url(),);

            let desc_suffix = format!("\n\n### {} ###", event.id());

            let desc = event.description().clone().replace('\r', "");
            let mut desc = re.replace_all(&desc, "").to_string();

            let mut truncate_at = 1000 - desc_prefix.len() - desc_suffix.len();
            while !desc.is_char_boundary(truncate_at) {
                truncate_at -= 1;
            }
            desc.truncate(truncate_at);

            let mut ev = CreateScheduledEvent::new(
                ScheduledEventType::External,
                event.name(),
                Timestamp::from(start_time.and_utc()),
            )
            .description(format!("{desc_prefix}{desc}{desc_suffix}"))
            .end_time(Timestamp::from(end_time.and_utc()))
            .location(event.departure().city())
            .audit_log_reason("Created from TruckersMP event");

            if let Some(banner) = event.banner() {
                if let Ok(img) = CreateAttachment::url(ctx.http(), banner).await {
                    ev = ev.image(&img);
                }
            }

            tracing::info!("Creating event for ID {}", event.id());
            guild_id
                .create_scheduled_event(&ctx, ev)
                .await
                .context("Failed to create new event")?;
        }

        Ok(())
    }
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: serenity::client::Context, ready: Ready) {
        tracing::info!("{} is connected!", ready.user.name);

        if ready.guilds.len() != 1 {
            tracing::error!("This only functions with a bot in only one guild.");
            process::exit(1);
        }

        // SAFETY: Length is checked prior
        let guild = ready.guilds.first().unwrap();
        let guild_id = guild.id;
        tracing::info!("Working on guild: {guild_id}");

        if let Err(e) = self.process_events(&guild_id, &ctx).await {
            eprintln!("{e:?}");
            process::exit(1);
        }

        process::exit(0);
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // a builder for `FmtSubscriber`.
    let subscriber = FmtSubscriber::builder()
        .with_env_filter(EnvFilter::from_default_env())
        // completes the builder.
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .context("setting default subscriber failed")?;

    dotenvy::dotenv()?;

    tracing::info!("Fetching events from TMP");

    // Fetch events from TMP
    let tmp_id = env::var("TMP_ID").context("Expected a TMP ID in the environment")?;
    let data_created: tmp_response::Response = reqwest::get(EVENT_API_URL.replace("{id}", &tmp_id))
        .await?
        .json()
        .await?;
    let data_attending: tmp_response::Response =
        reqwest::get(EVENT_ATTENDING_API_URL.replace("{id}", &tmp_id))
            .await?
            .json()
            .await?;

    if *data_created.error() || *data_attending.error() {
        tracing::error!("Error in returned data!");
        process::exit(1);
    }

    let mut data = data_created.response().clone();
    {
        // Append attending events
        let mut d = data_attending.response().clone();
        data.append(&mut d);
    }
    tracing::info!(
        "We have {} events from TruckersMP.",
        data_created.response().len() + data_attending.response().len()
    );

    // Login with a bot token from the environment
    tracing::info!("Connecting to Discord...");
    let token = env::var("DISCORD_TOKEN").context("Expected a token in the environment")?;

    // Create a new instance of the Client, logging in as a bot.
    let mut client = Client::builder(&token, GatewayIntents::empty())
        .event_handler(Handler { data })
        .await
        .context("Error creating Discord client")?;

    // Start listening for events by starting a single shard
    client.start().await?;
    tracing::info!("Disconnected");

    Ok(())
}
