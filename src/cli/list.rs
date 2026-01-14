//! `agent-of-empires list` command implementation

use anyhow::Result;
use clap::Args;
use serde::Serialize;

use crate::session::{Instance, Storage};

const TABLE_COL_TITLE: usize = 20;
const TABLE_COL_GROUP: usize = 15;
const TABLE_COL_PATH: usize = 40;
const TABLE_COL_ID_DISPLAY: usize = 12;

#[derive(Args)]
pub struct ListArgs {
    /// Output as JSON
    #[arg(long)]
    json: bool,

    /// List sessions from all profiles
    #[arg(long)]
    all: bool,
}

#[derive(Serialize)]
struct SessionJson {
    id: String,
    title: String,
    path: String,
    group: String,
    tool: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    command: String,
    profile: String,
    created_at: chrono::DateTime<chrono::Utc>,
}

fn print_table_header() {
    println!(
        "{:<width_title$} {:<width_group$} {:<width_path$} ID",
        "TITLE",
        "GROUP",
        "PATH",
        width_title = TABLE_COL_TITLE,
        width_group = TABLE_COL_GROUP,
        width_path = TABLE_COL_PATH
    );
    println!(
        "{}",
        "-".repeat(TABLE_COL_TITLE + TABLE_COL_GROUP + TABLE_COL_PATH + TABLE_COL_ID_DISPLAY + 5)
    );
}

fn print_table_row(inst: &Instance) {
    let title = super::truncate(&inst.title, TABLE_COL_TITLE);
    let group = super::truncate(&inst.group_path, TABLE_COL_GROUP);
    let path = super::truncate(&inst.project_path, TABLE_COL_PATH);
    let id_display = super::truncate_id(&inst.id, TABLE_COL_ID_DISPLAY);
    println!(
        "{:<width_title$} {:<width_group$} {:<width_path$} {}",
        title,
        group,
        path,
        id_display,
        width_title = TABLE_COL_TITLE,
        width_group = TABLE_COL_GROUP,
        width_path = TABLE_COL_PATH
    );
}

pub async fn run(profile: &str, args: ListArgs) -> Result<()> {
    if args.all {
        return run_all_profiles(args.json).await;
    }

    let storage = Storage::new(profile)?;
    let (instances, _) = storage.load_with_groups()?;

    if instances.is_empty() {
        println!("No sessions found in profile '{}'.", storage.profile());
        return Ok(());
    }

    if args.json {
        let sessions: Vec<SessionJson> = instances
            .iter()
            .map(|inst| SessionJson {
                id: inst.id.clone(),
                title: inst.title.clone(),
                path: inst.project_path.clone(),
                group: inst.group_path.clone(),
                tool: inst.tool.clone(),
                command: inst.command.clone(),
                profile: storage.profile().to_string(),
                created_at: inst.created_at,
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&sessions)?);
        return Ok(());
    }

    println!("Profile: {}\n", storage.profile());
    print_table_header();
    for inst in &instances {
        print_table_row(inst);
    }
    println!("\nTotal: {} sessions", instances.len());

    crate::update::print_update_notice().await;

    Ok(())
}

async fn run_all_profiles(json: bool) -> Result<()> {
    let profiles = crate::session::list_profiles()?;

    if profiles.is_empty() {
        println!("No profiles found.");
        return Ok(());
    }

    if json {
        let mut all_sessions: Vec<SessionJson> = Vec::new();
        for profile_name in &profiles {
            if let Ok(storage) = Storage::new(profile_name) {
                if let Ok((instances, _)) = storage.load_with_groups() {
                    for inst in instances {
                        all_sessions.push(SessionJson {
                            id: inst.id,
                            title: inst.title,
                            path: inst.project_path,
                            group: inst.group_path,
                            tool: inst.tool,
                            command: inst.command,
                            profile: profile_name.clone(),
                            created_at: inst.created_at,
                        });
                    }
                }
            }
        }
        println!("{}", serde_json::to_string_pretty(&all_sessions)?);
        return Ok(());
    }

    let mut total_sessions = 0;
    for profile_name in &profiles {
        if let Ok(storage) = Storage::new(profile_name) {
            if let Ok((instances, _)) = storage.load_with_groups() {
                if instances.is_empty() {
                    continue;
                }

                println!("\n═══ Profile: {} ═══\n", profile_name);
                print_table_header();
                for inst in &instances {
                    print_table_row(inst);
                }
                println!("({} sessions)", instances.len());
                total_sessions += instances.len();
            }
        }
    }

    println!("\n═══════════════════════════════════════");
    println!(
        "Total: {} sessions across {} profiles",
        total_sessions,
        profiles.len()
    );

    Ok(())
}
