use schedule::*;
use serde::Deserialize;
use std::fs;

use mysql::prelude::*;
use mysql::*;

#[allow(dead_code)]
#[derive(Deserialize)]
struct PubDataShift {
	name: String,
	friendly_name: String,
	description: String,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct PubDataLocation {
	name: String,
	friendly_name: String,
}

#[derive(Deserialize)]
struct PublicData {
	shifts: Vec<PubDataShift>,
	locations: Vec<PubDataLocation>,
}

fn main() {
	let contents = fs::read_to_string("../web/static/resources/json/PublicData.json")
		.expect("PublicData.json not found");
	let pubdata: PublicData = serde_json::from_str(&contents).unwrap();
	let mut conn = get_mysql_connection().unwrap();
	let w2: Vec<(
		String,
		String,
		String,
		String,
		String,
		String,
		String,
		String,
	)> = conn
		.query_map(
			"SELECT location, sun2, mon2, tue2, wed2, thu2, fri2, sat2 from schedule ORDER BY `id`",
			|(location, sun2, mon2, tue2, wed2, thu2, fri2, sat2)| {
				(location, sun2, mon2, tue2, wed2, thu2, fri2, sat2)
			},
		)
		.expect("Select error");
	if conn.query_drop("TRUNCATE schedule").is_err() {
		panic!("Truncate error");
	}
	let mut new_rows = vec![];
	let mut empty_day_cell_obj = vec![];

	for shift in pubdata.shifts {
		empty_day_cell_obj.push(ScheduleShift {
			name: shift.name.to_string(),
			bartender: String::new(),
		});
	}
	let newday = serde_json::to_string(&empty_day_cell_obj).unwrap();
	for location in pubdata.locations {
		for row in &w2 {
			if row.0 == location.name {
				new_rows.push(ScheduleRow {
					location: location.name.to_string(),
					sun1: row.1.to_string(),
					mon1: row.2.to_string(),
					tue1: row.3.to_string(),
					wed1: row.4.to_string(),
					thu1: row.5.to_string(),
					fri1: row.6.to_string(),
					sat1: row.7.to_string(),
					sun2: newday.to_string(),
					mon2: newday.to_string(),
					tue2: newday.to_string(),
					wed2: newday.to_string(),
					thu2: newday.to_string(),
					fri2: newday.to_string(),
					sat2: newday.to_string(),
				});
			}
		}
	}

	let sql_result = conn.exec_batch("INSERT INTO schedule (
		location, sun1, mon1, tue1, wed1, thu1, fri1, sat1, sun2, mon2, tue2, wed2, thu2, fri2, sat2
	) VALUES (
		:location, :sun1, :mon1, :tue1, :wed1, :thu1, :fri1, :sat1, :sun2, :mon2, :tue2, :wed2, :thu2, :fri2, :sat2
	)", new_rows.iter().map(|r| params! {
			"location" => &r.location,
			"sun1" => &r.sun1,
			"mon1" => &r.mon1,
			"tue1" => &r.tue1,
			"wed1" => &r.wed1,
			"thu1" => &r.thu1,
			"fri1" => &r.fri1,
			"sat1" => &r.sat1,
			"sun2" => &r.sun2,
			"mon2" => &r.mon2,
			"tue2" => &r.tue2,
			"wed2" => &r.wed2,
			"thu2" => &r.thu2,
			"fri2" => &r.fri2,
			"sat2" => &r.sat2,
		})
	);
	if sql_result.is_err() {
		panic!("Insert error");
	}
	println!("Week cycled successfully.");
}

fn get_mysql_connection() -> Result<PooledConn> {
	dotenvy::dotenv().ok();
	let pass = std::env::var("MYSQL_PASS").expect("Missing environment variable: MYSQL_PASS");
	let url: &str =
		&(String::from("mysql://kava:") + &pass + &String::from("@localhost:3306/kava"))[..];
	let pool = match Pool::new(url) {
		Ok(v) => v,
		Err(e) => return Err(e),
	};
	match pool.get_conn() {
		Ok(v) => Ok(v),
		Err(e) => Err(e),
	}
}
