//! `agent-of-empires group` subcommands implementation

use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use serde::Serialize;

use crate::session::{GroupTree, Storage};

#[derive(Subcommand)]
pub enum GroupCommands {
    /// List all groups
    #[command(alias = "ls")]
    List(GroupListArgs),

    /// Create a new group
    Create(GroupCreateArgs),

    /// Delete a group
    Delete(GroupDeleteArgs),

    /// Move session to group
    Move(GroupMoveArgs),
}

#[derive(Args)]
pub struct GroupListArgs {
    /// Output as JSON
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
pub struct GroupCreateArgs {
    /// Group name
    name: String,

    /// Parent group for creating subgroups
    #[arg(long)]
    parent: Option<String>,
}

#[derive(Args)]
pub struct GroupDeleteArgs {
    /// Group name
    name: String,

    /// Force delete by moving sessions to default group
    #[arg(long)]
    force: bool,
}

#[derive(Args)]
pub struct GroupMoveArgs {
    /// Session ID or title
    identifier: String,

    /// Target group
    group: String,
}

#[derive(Serialize)]
struct GroupInfo {
    name: String,
    path: String,
    session_count: usize,
    children: Vec<String>,
}

#[tracing::instrument(target = "cli.session", skip_all, fields(profile = %profile))]
pub async fn run(profile: &str, command: GroupCommands) -> Result<()> {
    match command {
        GroupCommands::List(args) => list_groups(profile, args).await,
        GroupCommands::Create(args) => create_group(profile, args).await,
        GroupCommands::Delete(args) => delete_group(profile, args).await,
        GroupCommands::Move(args) => move_session(profile, args).await,
    }
}

async fn list_groups(profile: &str, args: GroupListArgs) -> Result<()> {
    let storage = Storage::new(profile)?;
    let (instances, groups) = storage.load_with_groups()?;

    let group_tree = GroupTree::new_with_groups(&instances, &groups);

    if args.json {
        let group_list: Vec<GroupInfo> = group_tree
            .get_all_groups()
            .iter()
            .map(|g| {
                let session_count = instances.iter().filter(|i| i.group_path == g.path).count();
                GroupInfo {
                    name: g.name.clone(),
                    path: g.path.clone(),
                    session_count,
                    children: g.children.iter().map(|c| c.name.clone()).collect(),
                }
            })
            .collect();
        super::output::print_json(&group_list)?;
    } else {
        let all_groups = group_tree.get_all_groups();
        if all_groups.is_empty() {
            println!("No groups found.");
            println!("Create one with: aoe group create <name>");
            return Ok(());
        }

        println!("Groups:\n");
        for group in &all_groups {
            let session_count = instances
                .iter()
                .filter(|i| i.group_path == group.path)
                .count();
            let indent = group.path.matches('/').count();
            println!(
                "{}• {} ({} sessions)",
                "  ".repeat(indent),
                group.name,
                session_count
            );
        }
        println!("\nTotal: {} groups", all_groups.len());
    }

    Ok(())
}

async fn create_group(profile: &str, args: GroupCreateArgs) -> Result<()> {
    let storage = Storage::new(profile)?;

    let name = args.name.trim();
    let group_path = if let Some(parent) = &args.parent {
        format!("{}/{}", parent.trim(), name)
    } else {
        name.to_string()
    };

    // Persist through `update`, which re-loads under the cross-process lock.
    // The existence check runs against that fresh snapshot, not a stale one,
    // and the closure only adds the new group, so a concurrently-added group
    // or session row is never clobbered.
    let group_path_for_save = group_path.clone();
    storage.update(move |instances, groups| {
        let mut group_tree = GroupTree::new_with_groups(instances, groups);
        if group_tree.group_exists(&group_path_for_save) {
            bail!("Group already exists: {}", group_path_for_save);
        }
        group_tree.create_group(&group_path_for_save);
        *groups = group_tree.get_all_groups();
        Ok(())
    })?;

    println!("✓ Created group: {}", group_path);

    Ok(())
}

async fn delete_group(profile: &str, args: GroupDeleteArgs) -> Result<()> {
    let storage = Storage::new(profile)?;

    let name = args.name.trim().to_string();
    let force = args.force;

    // Persist through `update`: the existence check, the session count, and
    // the deletion all run against a fresh snapshot loaded under the
    // cross-process lock. The closure mutates only the target group and its
    // sessions, so a row another process added concurrently is preserved.
    // The count is returned so the post-update messages stay accurate.
    let name_for_save = name.clone();
    let session_count = storage.update(move |instances, groups| {
        let mut group_tree = GroupTree::new_with_groups(instances, groups);

        if !group_tree.group_exists(&name_for_save) {
            bail!("Group not found: {}", name_for_save);
        }

        let prefix = format!("{}/", name_for_save);
        let session_count = instances
            .iter()
            .filter(|i| i.group_path == name_for_save || i.group_path.starts_with(&prefix))
            .count();

        if session_count > 0 {
            if !force {
                bail!(
                    "Group '{}' contains {} sessions. Use --force to move them to default group.",
                    name_for_save,
                    session_count
                );
            }

            // Move sessions to default group
            for inst in instances.iter_mut() {
                if inst.group_path == name_for_save || inst.group_path.starts_with(&prefix) {
                    inst.group_path = String::new();
                }
            }
        }

        group_tree.delete_group(&name_for_save);
        *groups = group_tree.get_all_groups();
        Ok(session_count)
    })?;

    println!("✓ Deleted group: {}", name);
    if force && session_count > 0 {
        println!("  Moved {} sessions to default group", session_count);
    }

    Ok(())
}

async fn move_session(profile: &str, args: GroupMoveArgs) -> Result<()> {
    let storage = Storage::new(profile)?;

    let identifier = args.identifier.trim().to_string();
    let group = args.group.trim().to_string();

    // Persist through `update`: the session lookup and the group reassignment
    // both run against a fresh snapshot loaded under the cross-process lock.
    // The closure touches only the target row's `group_path` and adds the
    // destination group, so a concurrently-added row is never clobbered. The
    // previous group is returned so the post-update message stays accurate.
    let identifier_for_save = identifier.clone();
    let group_for_save = group.clone();
    let old_group = storage.update(move |instances, groups| {
        let inst = instances
            .iter_mut()
            .find(|i| {
                i.id == identifier_for_save
                    || i.id.starts_with(&identifier_for_save)
                    || i.title == identifier_for_save
            })
            .ok_or_else(|| anyhow::anyhow!("Session not found: {}", identifier_for_save))?;

        let old_group = inst.group_path.clone();
        inst.group_path = group_for_save.clone();

        if !group_for_save.is_empty() {
            let mut group_tree = GroupTree::new_with_groups(instances, groups);
            group_tree.create_group(&group_for_save);
            *groups = group_tree.get_all_groups();
        }
        Ok(old_group)
    })?;

    if old_group.is_empty() {
        println!("✓ Moved session to group: {}", group);
    } else {
        println!("✓ Moved session from '{}' to '{}'", old_group, group);
    }

    Ok(())
}
