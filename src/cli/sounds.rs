//! `agent-of-empires sounds` subcommands implementation

use anyhow::Result;
use clap::Subcommand;

use crate::sound;

#[derive(Subcommand)]
pub enum SoundsCommands {
    /// Install bundled sound effects
    Install,

    /// List currently installed sounds
    #[command(alias = "ls")]
    List,

    /// Test a sound by playing it
    Test {
        /// Sound file name (without extension)
        name: String,
    },
}

pub async fn run(command: SoundsCommands) -> Result<()> {
    match command {
        SoundsCommands::Install => install_bundled().await,
        SoundsCommands::List => list_sounds(),
        SoundsCommands::Test { name } => test_sound(&name),
    }
}

async fn install_bundled() -> Result<()> {
    println!("📥 Downloading bundled CC0 sounds from GitHub...\n");

    match sound::install_bundled_sounds().await {
        Ok(()) => {
            if let Some(sounds_dir) = sound::get_sounds_dir() {
                println!("\n✓ Successfully installed bundled CC0 sounds to:");
                println!("  {}\n", sounds_dir.display());

                let sounds = sound::list_available_sounds();
                println!("📂 Installed {} sounds:", sounds.len());
                for sound_name in sounds {
                    println!("  • {}", sound_name);
                }

                println!("\n💡 Next steps:");
                println!("  1. Launch the TUI: aoe");
                println!("  2. Press 's' to open Settings");
                println!("  3. Navigate to Sound category");
                println!("  4. Enable sounds and configure transitions");

                println!("\n🎮 Want Age of Empires II sounds instead?");
                println!("   If you own AoE II, copy the taunt .wav files from:");
                println!("   • (AoE II dir)/resources/_common/sound/taunt/");
                println!("   • Or: (AoE II dir)/Sound/taunt/");
                println!("   To: {}", sounds_dir.display());
                println!("\n   Then configure which sounds to use in Settings!");
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("\n❌ Failed to install sounds: {}", e);
            eprintln!("\n💡 Troubleshooting:");
            eprintln!("  • Check your internet connection");
            eprintln!("  • Try again later if GitHub is unavailable");
            eprintln!("  • You can manually download sounds from:");
            eprintln!("    https://github.com/njbrake/agent-of-empires/tree/main/bundled_sounds");
            Err(e)
        }
    }
}

fn list_sounds() -> Result<()> {
    let sounds = sound::list_available_sounds();

    if sounds.is_empty() {
        println!("No sounds installed yet.");
        println!("\nRun 'aoe sounds install' to get started.");
        return Ok(());
    }

    println!("📂 Installed sounds:");
    for sound_name in &sounds {
        println!("  • {}", sound_name);
    }
    println!("\nTotal: {} sounds", sounds.len());

    if let Some(sounds_dir) = sound::get_sounds_dir() {
        println!("\nLocation: {}", sounds_dir.display());
    }

    println!("\n💡 Test a sound: aoe sounds test <name>");

    Ok(())
}

fn test_sound(name: &str) -> Result<()> {
    let sounds = sound::list_available_sounds();

    if !sounds.contains(&name.to_string()) {
        println!("❌ Sound '{}' not found.", name);
        println!("\n📂 Available sounds:");
        for sound_name in sounds {
            println!("  • {}", sound_name);
        }
        return Ok(());
    }

    print!("🔊 Playing '{}'... ", name);
    std::io::Write::flush(&mut std::io::stdout())?;

    match sound::play_sound_blocking(name) {
        Ok(()) => {
            println!("✓");
            Ok(())
        }
        Err(e) => {
            println!("✗");
            eprintln!("\n❌ Failed to play sound: {}", e);
            eprintln!("\n💡 Troubleshooting:");
            if cfg!(target_os = "linux") {
                eprintln!("  • Ensure audio tools are installed:");
                eprintln!("    - Debian/Ubuntu: sudo apt install alsa-utils pulseaudio-utils");
                eprintln!("    - Arch: sudo pacman -S alsa-utils pulseaudio");
                eprintln!("  • Check that your audio device is working");
                eprintln!("  • Note: Audio doesn't work over SSH sessions");
            } else {
                eprintln!("  • Check that your audio device is working");
                eprintln!("  • Note: Audio doesn't work over SSH sessions");
            }
            Err(e.into())
        }
    }
}
