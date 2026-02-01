//! Task management CLI commands

use anyhow::Result;
use clap::{Args, Subcommand};
use std::path::PathBuf;

use crate::task::{Task, TaskId, TaskPriority, TaskStatus, TasksFile};

#[derive(Subcommand)]
pub enum TaskCommands {
    /// List tasks
    List(TaskListArgs),

    /// Add a new task
    Add(TaskAddArgs),

    /// Show task details
    Show(TaskShowArgs),

    /// Mark task as done
    Done(TaskDoneArgs),

    /// Update task status
    Update(TaskUpdateArgs),
}

#[derive(Args)]
pub struct TaskListArgs {
    /// Filter by status (todo, active, blocked, done)
    #[arg(short, long)]
    status: Option<String>,

    /// Filter by project
    #[arg(short, long)]
    project: Option<String>,

    /// Show overdue tasks only
    #[arg(long)]
    overdue: bool,

    /// Show tasks due today
    #[arg(long)]
    today: bool,

    /// Path to TASKS.md
    #[arg(long)]
    file: Option<PathBuf>,
}

#[derive(Args)]
pub struct TaskAddArgs {
    /// Task title
    title: String,

    /// Priority (low, medium, high, urgent)
    #[arg(short, long, default_value = "medium")]
    priority: String,

    /// Due date (YYYY-MM-DD)
    #[arg(short, long)]
    due: Option<String>,

    /// Associated project
    #[arg(long)]
    project: Option<String>,

    /// Path to TASKS.md
    #[arg(long)]
    file: Option<PathBuf>,
}

#[derive(Args)]
pub struct TaskShowArgs {
    /// Task ID (e.g., T001)
    id: String,

    /// Path to TASKS.md
    #[arg(long)]
    file: Option<PathBuf>,
}

#[derive(Args)]
pub struct TaskDoneArgs {
    /// Task ID (e.g., T001)
    id: String,

    /// Path to TASKS.md
    #[arg(long)]
    file: Option<PathBuf>,
}

#[derive(Args)]
pub struct TaskUpdateArgs {
    /// Task ID (e.g., T001)
    id: String,

    /// New status (todo, active, blocked, done)
    #[arg(short, long)]
    status: Option<String>,

    /// New priority
    #[arg(short, long)]
    priority: Option<String>,

    /// Path to TASKS.md
    #[arg(long)]
    file: Option<PathBuf>,
}

pub async fn run(command: TaskCommands) -> Result<()> {
    match command {
        TaskCommands::List(args) => run_list(args).await,
        TaskCommands::Add(args) => run_add(args).await,
        TaskCommands::Show(args) => run_show(args).await,
        TaskCommands::Done(args) => run_done(args).await,
        TaskCommands::Update(args) => run_update(args).await,
    }
}

fn get_tasks_path(file: Option<PathBuf>) -> PathBuf {
    file.unwrap_or_else(|| {
        dirs::home_dir()
            .map(|h| h.join("clawd").join("TASKS.md"))
            .unwrap_or_else(|| PathBuf::from("TASKS.md"))
    })
}

async fn run_list(args: TaskListArgs) -> Result<()> {
    let path = get_tasks_path(args.file);

    if !path.exists() {
        println!("No TASKS.md found at {:?}", path);
        return Ok(());
    }

    let file = TasksFile::from_file(&path)?;

    let status_filter = args.status.as_ref().and_then(|s| TaskStatus::parse(s));

    let mut tasks: Vec<&Task> = if args.overdue {
        file.overdue()
    } else if args.today {
        file.due_today()
    } else if let Some(status) = status_filter {
        file.by_status(status)
    } else {
        file.all_tasks()
    };

    // Filter by project
    if let Some(project) = &args.project {
        tasks.retain(|t| t.project.as_ref().map(|p| p == project).unwrap_or(false));
    }

    if tasks.is_empty() {
        println!("No tasks found");
        return Ok(());
    }

    println!("Tasks ({}):\n", tasks.len());

    for task in tasks {
        println!("{}", task.to_markdown_line());
    }

    Ok(())
}

async fn run_add(args: TaskAddArgs) -> Result<()> {
    let path = get_tasks_path(args.file);

    let mut file = if path.exists() {
        TasksFile::from_file(&path)?
    } else {
        TasksFile {
            sections: vec![
                crate::task::parser::TaskSection {
                    title: "Active".to_string(),
                    tasks: vec![],
                },
                crate::task::parser::TaskSection {
                    title: "Todo".to_string(),
                    tasks: vec![],
                },
                crate::task::parser::TaskSection {
                    title: "Done".to_string(),
                    tasks: vec![],
                },
            ],
        }
    };

    let id = file.next_id();
    let mut task = Task::new(id.clone(), &args.title);

    task.priority = TaskPriority::parse(&args.priority).unwrap_or_default();
    task.project = args.project;

    if let Some(due_str) = &args.due {
        task.due = chrono::NaiveDate::parse_from_str(due_str, "%Y-%m-%d").ok();
    }

    // Add to Todo section
    if let Some(section) = file.sections.iter_mut().find(|s| s.title == "Todo") {
        section.tasks.push(task.clone());
    }

    file.write_to_file(&path)?;

    println!("Created: {}", task.to_markdown_line());

    Ok(())
}

async fn run_show(args: TaskShowArgs) -> Result<()> {
    let path = get_tasks_path(args.file);
    let file = TasksFile::from_file(&path)?;

    let id = TaskId::parse(&args.id).ok_or_else(|| anyhow::anyhow!("Invalid task ID"))?;

    match file.get(&id) {
        Some(task) => {
            println!("{} {}: {}", task.status.emoji(), task.id, task.title);
            println!("  Status: {}", task.status.label());
            println!("  Priority: {}", task.priority.label());

            if let Some(project) = &task.project {
                println!("  Project: {}", project);
            }

            if let Some(due) = &task.due {
                let overdue = task.is_overdue();
                println!(
                    "  Due: {}{}",
                    due.format("%Y-%m-%d"),
                    if overdue { " ⚠️ OVERDUE" } else { "" }
                );
            }

            if !task.notes.is_empty() {
                println!("  Notes:");
                for note in &task.notes {
                    println!("    - {}", note);
                }
            }

            Ok(())
        }
        None => {
            anyhow::bail!("Task not found: {}", args.id);
        }
    }
}

async fn run_done(args: TaskDoneArgs) -> Result<()> {
    let path = get_tasks_path(args.file);
    let mut file = TasksFile::from_file(&path)?;

    let id = TaskId::parse(&args.id).ok_or_else(|| anyhow::anyhow!("Invalid task ID"))?;

    // Find and update the task
    let mut found = false;
    for section in &mut file.sections {
        if let Some(task) = section.tasks.iter_mut().find(|t| t.id == id) {
            task.complete();
            found = true;
            println!("Completed: {}", task.to_markdown_line());
            break;
        }
    }

    if !found {
        anyhow::bail!("Task not found: {}", args.id);
    }

    file.write_to_file(&path)?;

    Ok(())
}

async fn run_update(args: TaskUpdateArgs) -> Result<()> {
    let path = get_tasks_path(args.file);
    let mut file = TasksFile::from_file(&path)?;

    let id = TaskId::parse(&args.id).ok_or_else(|| anyhow::anyhow!("Invalid task ID"))?;

    // Find and update the task
    let mut found = false;
    for section in &mut file.sections {
        if let Some(task) = section.tasks.iter_mut().find(|t| t.id == id) {
            if let Some(status_str) = &args.status {
                if let Some(status) = TaskStatus::parse(status_str) {
                    task.status = status;
                }
            }

            if let Some(priority_str) = &args.priority {
                if let Some(priority) = TaskPriority::parse(priority_str) {
                    task.priority = priority;
                }
            }

            task.touch();
            found = true;
            println!("Updated: {}", task.to_markdown_line());
            break;
        }
    }

    if !found {
        anyhow::bail!("Task not found: {}", args.id);
    }

    file.write_to_file(&path)?;

    Ok(())
}
