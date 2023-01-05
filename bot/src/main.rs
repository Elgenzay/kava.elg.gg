mod cmds;

use chrono::{Datelike, TimeZone, Utc, Weekday};
use discord_log::Logger;
use kava_mysql::{get_mysql_connection, mysql_is_running};
use mysql::prelude::Queryable;
use serde::{Deserialize, Serialize};
use serenity::async_trait;
use serenity::client::bridge::gateway::ShardManager;
use serenity::framework::standard::macros::{command, group};
use serenity::framework::standard::{CommandResult, StandardFramework};
use serenity::model::channel::{Message, Reaction, ReactionType};
use serenity::model::gateway::Ready;
use serenity::model::guild::Member;
use serenity::model::prelude::command::Command;
use serenity::model::prelude::interaction::{Interaction, InteractionResponseType};
use serenity::model::prelude::{ChannelId, GuildChannel, GuildId, RoleId, UserId};
use serenity::prelude::Mutex;
use serenity::prelude::*;
use serenity::utils::ArgumentConvert;
use std::fs;
use std::sync::Arc;
use std::time::Duration;
use tokio::{task, time};

const TICKRATE_SECONDS: u64 = 10;
const OFFSET_HOURS: i64 = -3;

struct ShardManagerContainer;

impl TypeMapKey for ShardManagerContainer {
	type Value = Arc<Mutex<ShardManager>>;
}

struct BotData;

impl TypeMapKey for BotData {
	type Value = BotState;
}

#[derive(Clone)]
struct BotState {
	initialized: bool,
	data: JsonData,
	logger: Logger,
	weekday: Weekday,
}

impl BotState {
	fn new() -> BotState {
		BotState {
			initialized: false,
			data: JsonData {
				react_role_groups: Vec::new(),
			},
			logger: Logger::new(),
			weekday: get_offset_weekday(),
		}
	}
}

#[derive(Serialize, Deserialize, Clone)]
struct JsonData {
	react_role_groups: Vec<ReactRoleGroup>,
}

#[derive(Serialize, Deserialize, Clone)]
struct ReactRoleGroup {
	message_id: u64,
	mutually_exclusive: bool,
	roles: Vec<ReactRole>,
}

#[derive(Serialize, Deserialize, Clone)]
struct ReactRole {
	emoji: String,
	role_id: u64,
}

#[group]
#[commands(ping)]
struct General;

struct Handler;

#[async_trait]
impl EventHandler for Handler {
	async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
		if let Interaction::ApplicationCommand(command) = interaction {
			let content = match command.data.name.as_str() {
				"ping" => cmds::ping::run(&command.data.options),
				"debug" => cmds::debug::run(&command.data.options),
				_ => "".to_string(),
			};
			if let Err(e) = command
				.create_interaction_response(&ctx.http, |response| {
					response
						.kind(InteractionResponseType::ChannelMessageWithSource)
						.interaction_response_data(|message| message.content(content))
				})
				.await
			{
				get_state(&ctx).await.logger.log_error(format!(
					"Error responding to command /{}: {}",
					command.data.name.as_str(),
					e
				));
			}
		}
	}

	async fn message(&self, _ctx: Context, msg: Message) {
		if msg.content == "k!cycle" && msg.author.id == UserId(97802694302896128) {
			schedule_notify::weekly();
		}
	}

	async fn reaction_add(&self, ctx: Context, react: Reaction) {
		reaction_update(ctx, react, true).await;
	}

	async fn reaction_remove(&self, ctx: Context, react: Reaction) {
		reaction_update(ctx, react, false).await;
	}

	async fn ready(&self, ctx: Context, ready: Ready) {
		println!("{} is connected!", ready.user.name);
		let state = get_state(&ctx).await;
		let cmd_register = Command::set_global_application_commands(&ctx.http, |commands| {
			commands
				.create_application_command(|command| cmds::ping::register(command))
				.create_application_command(|command| cmds::debug::register(command))
		})
		.await;
		if let Err(e) = cmd_register {
			state
				.logger
				.panic(format!("Error registering commands: {}", e.to_string()));
		};
		if task::spawn(async move {
			let mut interval = time::interval(Duration::from_millis(TICKRATE_SECONDS * 1000));
			loop {
				interval.tick().await;
				tick(&ctx).await;
			}
		})
		.await
		.is_err()
		{
			state.logger.panic("Tokio task spawn failure".to_string());
		}
	}
}

async fn reaction_update(ctx: Context, react: Reaction, adding: bool) {
	let result =
		async {
			let groups = get_state(&ctx).await.data.react_role_groups;
			let msg_id = react.message_id.as_u64();
			let mut match_group_opt = None;
			for group in groups {
				if &group.message_id == msg_id {
					match_group_opt = Some(group.clone());
					break;
				}
			}
			let match_group = match match_group_opt {
				Some(v) => v,
				None => return Ok(()),
			};
			let reaction_str = match &react.emoji {
				ReactionType::Unicode(s) => s,
				_ => return Ok(()),
			};
			let mut role_id = None;
			let mut remove_role_ids = vec![];
			for reactrole in match_group.roles {
				if reaction_str == &reactrole.emoji {
					role_id = Some(reactrole.role_id);
				} else {
					remove_role_ids.push(reactrole.role_id);
				}
			}
			if role_id.is_none() {
				return Ok(());
			}
			let user_id = match &react.user_id {
				Some(v) => v,
				None => return Ok(()),
			};
			let user_id_str = &user_id.to_string()[..];
			let mut member =
				match Member::convert(&ctx, react.guild_id, Some(react.channel_id), user_id_str)
					.await
				{
					Ok(v) => v,
					Err(e) => return Err(e.to_string()),
				};
			if match_group.mutually_exclusive {
				for remove_role in remove_role_ids {
					if let Err(e) = member.remove_role(&ctx.http, RoleId(remove_role)).await {
						return Err(e.to_string());
					};
				}
			}
			if adding {
				if let Err(e) = member.add_role(&ctx.http, RoleId(role_id.unwrap())).await {
					return Err(e.to_string());
				};
			} else {
				if let Err(e) = member
					.remove_role(&ctx.http, RoleId(role_id.unwrap()))
					.await
				{
					return Err(e.to_string());
				};
			}
			Ok(())
		}
		.await;
	if result.is_err() {
		get_state(&ctx).await.logger.log_error(format!(
			"Error on reaction update: {}",
			result.err().unwrap()
		));
	}
}

async fn get_state(ctx: &Context) -> BotState {
	let data = ctx.data.read().await;
	let state = data.get::<BotData>().unwrap();
	if state.initialized {
		return state.clone();
	}
	std::mem::drop(data);
	reset_state(ctx).await
}

async fn reset_state(ctx: &Context) -> BotState {
	let mut data = ctx.data.write().await;
	let state = data.get_mut::<BotData>().unwrap();
	state.weekday = get_offset_weekday();
	state.initialized = true;
	let json_str = fs::read_to_string("BotConfig.json").expect("Error reading BotConfig.json");
	state.data = serde_json::from_str(&json_str).expect("Error parsing BotConfig.json");
	state.clone()
}

#[tokio::main]
async fn main() {
	dotenvy::dotenv().ok();
	let token = std::env::var("BOT_TOKEN").expect("Missing environment variable: BOT_TOKEN");
	let intents = GatewayIntents::non_privileged()
		| GatewayIntents::GUILD_MESSAGES
		| GatewayIntents::GUILD_MESSAGE_REACTIONS
		| GatewayIntents::MESSAGE_CONTENT;
	let framework = StandardFramework::new()
		.configure(|c| c.prefix("k!"))
		.group(&GENERAL_GROUP);
	let mut client = Client::builder(token, intents)
		.event_handler(Handler)
		.framework(framework)
		.type_map_insert::<BotData>(BotState::new())
		.await
		.expect("Error creating client");
	if let Err(e) = client.start().await {
		println!("Client error: {}", e.to_string());
	}
}

fn get_offset_weekday() -> Weekday {
	Utc.timestamp_opt(
		chrono::offset::Local::now().timestamp() + (3600 * OFFSET_HOURS),
		0,
	)
	.unwrap()
	.date_naive()
	.weekday()
}

async fn tick(ctx: &Context) {
	if !mysql_is_running() {
		return;
	}
	let state = get_state(&ctx).await;
	if get_offset_weekday() != state.weekday {
		schedule_notify::daily();
		if reset_state(ctx).await.weekday == Weekday::Sat {
			schedule_notify::weekly();
		}
	}
	let mut conn = get_mysql_connection();
	let rows: Vec<(i64, u64, u64, String, String)> = conn
		.query("SELECT id, guild_id, ch_id, msg, reactions FROM log_queue LIMIT 1")
		.unwrap();
	if rows.len() == 0 {
		return;
	}
	let row = rows.first().unwrap();
	match conn.exec_drop("DELETE FROM log_queue WHERE id=?", (row.0,)) {
		Ok(_) => (),
		Err(e) => println!("MySQL delete error: {}", e.to_string()),
	}
	let guild_channel = GuildChannel::convert(
		ctx,
		Some(GuildId(row.1)),
		Some(ChannelId(row.2)),
		&row.2.to_string()[..],
	)
	.await;

	match guild_channel {
		Ok(v) => match v.send_message(&ctx.http, |m| m.content(&row.3)).await {
			Ok(msg) => {
				let reactions: Vec<String> = serde_json::from_str(&row.4).unwrap();
				for reaction in reactions {
					msg.react(&ctx.http, ReactionType::Unicode(reaction.to_string()))
						.await
						.unwrap();
				}
				()
			}
			Err(e) => println!("Error sending message: {}", e.to_string()),
		},
		Err(e) => println!(
			"Error finding guild channel from log_queue: {}",
			e.to_string()
		),
	}
}

#[command]
async fn ping(ctx: &Context, msg: &Message) -> CommandResult {
	msg.reply(ctx, "Pong!").await?;
	Ok(())
}
