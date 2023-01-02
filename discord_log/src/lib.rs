use mysql::{prelude::Queryable, Pool, PooledConn};

#[derive(Clone)]
pub struct Logger {
	guild_id: String,
	ch_id_error: String,
	ch_id_generic: String,
}

impl Logger {
	pub fn new() -> Logger {
		dotenvy::dotenv().ok();
		Logger {
			guild_id: std::env::var("DISCORD_GUILD_ID")
				.expect("Missing environment variable: DISCORD_LOG_CHANNEL_ID"),
			ch_id_error: std::env::var("DISCORD_ERROR_CHANNEL_ID")
				.expect("Missing environment variable: DISCORD_ERROR_CHANNEL_ID"),
			ch_id_generic: std::env::var("DISCORD_LOG_CHANNEL_ID")
				.expect("Missing environment variable: DISCORD_LOG_CHANNEL_ID"),
		}
	}

	pub fn panic(&self, msg: String) {
		self.log_error(msg.to_string());
		panic!("{}", msg);
	}

	pub fn log_error(&self, msg: String) {
		self.log(msg, &self.guild_id, &self.ch_id_error)
	}

	pub fn log_message(&self, msg: String) {
		self.log(msg, &self.guild_id, &self.ch_id_generic)
	}

	fn log(&self, msg: String, guild_id: &String, ch_id: &String) {
		match get_mysql_connection().exec_drop(
			"INSERT INTO log_queue (guild_id, ch_id, msg) VALUES (?,?,?)",
			(guild_id, ch_id, msg),
		) {
			Ok(_) => (),
			Err(e) => println!("Insert error (nonfatal): {}", e.to_string()),
		}
	}
}

fn get_mysql_connection() -> PooledConn {
	let pass = std::env::var("MYSQL_PASS").expect("Missing environment variable: MYSQL_PASS");
	let url: &str =
		&(String::from("mysql://kava:") + &pass + &String::from("@localhost:3306/kava"))[..];
	let pool = match Pool::new(url) {
		Ok(v) => v,
		Err(e) => panic!("{}", e.to_string()),
	};
	match pool.get_conn() {
		Ok(v) => v,
		Err(e) => panic!("{}", e.to_string()),
	}
}
