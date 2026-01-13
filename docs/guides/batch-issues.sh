#!/bin/bash

# Script to fetch GitHub issues from a repository and launch aoe sessions for each

set -e

# Default values
REPO=""
REPO_PATH=""
PROFILE=""
GROUP=""
SANDBOX_IMAGE=""
DRY_RUN=false
LIMIT=0  # 0 means no limit

AOE_CMD="./target/release/aoe"

# Parse command line arguments
usage() {
    echo "Usage: $0 --repo OWNER/REPO --path PATH [OPTIONS]"
    echo ""
    echo "Fetch GitHub issues from a repository and launch aoe sessions for each."
    echo ""
    echo "Required:"
    echo "  --repo OWNER/REPO   GitHub repository (e.g., mozilla-ai/any-llm)"
    echo "  --path PATH         Path to the local repository clone"
    echo ""
    echo "Options:"
    echo "  -p, --profile NAME  aoe profile to use (default: derived from repo name)"
    echo "  -g, --group NAME    Session group name (default: derived from repo name)"
    echo "  -s, --sandbox IMG   Docker sandbox image (optional, runs without sandbox if not set)"
    echo "  -n, --dry-run       Show what would be done without creating sessions"
    echo "  -l, --limit NUM     Limit the number of issues to process (default: all)"
    echo "  -h, --help          Show this help message"
    echo ""
    echo "Examples:"
    echo "  $0 --repo mozilla-ai/any-llm --path ~/scm/any-llm"
    echo "  $0 --repo myorg/myrepo --path ./myrepo --profile myprofile --limit 5"
    echo "  $0 --repo myorg/myrepo --path ./myrepo --sandbox my-sandbox:latest"
    exit 0
}

while [[ $# -gt 0 ]]; do
    case $1 in
        --repo)
            REPO="$2"
            shift 2
            ;;
        --path)
            REPO_PATH="$2"
            shift 2
            ;;
        -p|--profile)
            PROFILE="$2"
            shift 2
            ;;
        -g|--group)
            GROUP="$2"
            shift 2
            ;;
        -s|--sandbox)
            SANDBOX_IMAGE="$2"
            shift 2
            ;;
        -n|--dry-run)
            DRY_RUN=true
            shift
            ;;
        -l|--limit)
            LIMIT="$2"
            shift 2
            ;;
        -h|--help)
            usage
            ;;
        *)
            echo "Unknown option: $1"
            usage
            ;;
    esac
done

# Validate required arguments
if [ -z "$REPO" ]; then
    echo "Error: --repo is required"
    echo ""
    usage
fi

if [ -z "$REPO_PATH" ]; then
    echo "Error: --path is required"
    echo ""
    usage
fi

# Derive defaults from repo name if not specified
REPO_NAME=$(echo "$REPO" | sed 's|.*/||' | tr '-' '_')
if [ -z "$PROFILE" ]; then
    PROFILE="$REPO_NAME"
fi
if [ -z "$GROUP" ]; then
    GROUP="$REPO_NAME"
fi

# Verify the repository exists
if [ ! -d "$REPO_PATH" ]; then
    echo "Error: Repository not found at $REPO_PATH"
    echo "Please clone the repository first"
    exit 1
fi

echo "GitHub repo:     $REPO"
echo "Local path:      $REPO_PATH"
echo "Profile:         $PROFILE"
echo "Group:           $GROUP"
if [ -n "$SANDBOX_IMAGE" ]; then
    echo "Sandbox:         $SANDBOX_IMAGE"
fi
if [ "$DRY_RUN" = true ]; then
    echo "DRY RUN MODE - no sessions will be created"
fi

# Fetch all open issues from the repository using gh CLI
echo "Fetching open issues from $REPO..."
issues=$(gh issue list --repo "$REPO" --state open --json number,url --limit 1000)

# Check if we got any issues
issue_count=$(echo "$issues" | jq length)
echo "Found $issue_count open issues"

if [ "$issue_count" -eq 0 ]; then
    echo "No open issues found. Exiting."
    exit 0
fi

# Apply limit if specified
if [ "$LIMIT" -gt 0 ]; then
    echo "Limiting to first $LIMIT issues"
    issues=$(echo "$issues" | jq ".[:$LIMIT]")
    issue_count=$(echo "$issues" | jq length)
fi

# Get existing sessions to check for duplicates
echo "Checking for existing aoe sessions..."
existing_sessions=$($AOE_CMD list --profile "$PROFILE" --json 2>/dev/null || echo "[]")

# Function to check if a session already exists
session_exists() {
    local session_name="$1"
    echo "$existing_sessions" | jq -e --arg name "$session_name" '.[] | select(.title == $name)' > /dev/null 2>&1
}

# Function to check if a git branch exists
branch_exists() {
    local branch_name="$1"
    git -C "$REPO_PATH" show-ref --verify --quiet "refs/heads/$branch_name" 2>/dev/null
}

echo "Creating sessions in group: $GROUP"
echo ""

# Counters (use temp files to persist across subshell)
skipped_file=$(mktemp)
launched_file=$(mktemp)
echo "0" > "$skipped_file"
echo "0" > "$launched_file"
trap "rm -f $skipped_file $launched_file" EXIT

# Loop through each issue
echo "$issues" | jq -c '.[]' | while read -r issue; do
    number=$(echo "$issue" | jq -r '.number')
    url=$(echo "$issue" | jq -r '.url')
    session_name="ISSUE-$number"

    echo "Processing issue #$number: $url"

    # Check if session already exists
    if session_exists "$session_name"; then
        echo "  SKIPPED: Session '$session_name' already exists"
        echo $(( $(cat "$skipped_file") + 1 )) > "$skipped_file"
        echo ""
        continue
    fi

    # Build the prompt for Claude
    prompt="please look at $url: for this issue, do you have any recommendations about how to resolve the issue? If the issue cannot or should not be resolved, please indicate what action should be taken with the github issue"

    if [ "$DRY_RUN" = true ]; then
        echo "  [DRY RUN] Would create session:"
        echo "    Title:    $session_name"
        echo "    Worktree: issue_$number"
        echo "    Profile:  $PROFILE"
        echo "    Group:    $GROUP"
        if [ -n "$SANDBOX_IMAGE" ]; then
            echo "    Sandbox:  $SANDBOX_IMAGE"
        fi
        echo "    Command:  claude --dangerously-skip-permissions --permission-mode plan \"...\""
        echo ""
        echo $(( $(cat "$launched_file") + 1 )) > "$launched_file"
    else
        # Check if branch already exists
        branch_name="issue_$number"
        new_branch_flag=""
        if branch_exists "$branch_name"; then
            echo "  Note: Branch '$branch_name' already exists, reusing it"
        else
            new_branch_flag="--new-branch"
        fi

        # Build sandbox flag if specified
        sandbox_flag=""
        if [ -n "$SANDBOX_IMAGE" ]; then
            sandbox_flag="--sandbox-image $SANDBOX_IMAGE"
        fi

        # Create aoe session with worktree and Claude in plan mode
        if $AOE_CMD add "$REPO_PATH" \
            --worktree "$branch_name" \
            $new_branch_flag \
            --profile "$PROFILE" \
            --title "$session_name" \
            --sandbox \
            $sandbox_flag \
            --group "$GROUP" \
            --cmd "claude --dangerously-skip-permissions --permission-mode plan \"$prompt\""; then
            # Start the session (without attaching)
            if $AOE_CMD -p "$PROFILE" session start "$session_name"; then
                # Wait for the confirmation prompt to appear, then send Enter to accept
                sleep 2
                # Find the actual tmux session name (includes a random ID suffix)
                tmux_session=$(tmux list-sessions -F '#{session_name}' 2>/dev/null | grep "^aoe_${session_name}" | head -1)
                if [ -n "$tmux_session" ]; then
                    # Select "Yes, I accept" (option 2) then confirm
                    tmux send-keys -t "$tmux_session" Down Enter
                fi
                echo "Created and started session for issue #$number"
                echo $(( $(cat "$launched_file") + 1 )) > "$launched_file"
            else
                echo "  WARNING: Created session but failed to start for issue #$number"
            fi
        else
            echo "  WARNING: Failed to create session for issue #$number"
        fi
        echo ""
        # Small delay between launches
        sleep 1
    fi
done

launched=$(cat "$launched_file")
skipped=$(cat "$skipped_file")

echo "========================================"
echo "Summary:"
echo "  Launched: $launched"
echo "  Skipped:  $skipped (already exist)"
echo ""
if [ "$DRY_RUN" = true ]; then
    echo "DRY RUN complete. Run without --dry-run to actually create sessions."
else
    echo "Done! View sessions with: $AOE_CMD -p $PROFILE"
fi
