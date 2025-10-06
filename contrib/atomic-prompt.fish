# Atomic VCS Prompt Integration for Fish Shell
#
# This script provides shell prompt integration for Atomic VCS,
# displaying the current channel in your terminal prompt.
#
# Installation:
#   1. Copy this file to ~/.config/fish/functions/fish_prompt.fish
#      OR source it in your ~/.config/fish/config.fish:
#      source /path/to/atomic-prompt.fish
#
#   2. Use the atomic_prompt function in your prompt:
#      Add to your fish_prompt function:
#      echo -n (atomic_prompt)
#
# Configuration:
#   Set these environment variables to customize the prompt:
#   - ATOMIC_PROMPT_FORMAT: Custom format string (default: "[{channel}]")
#   - ATOMIC_PROMPT_COLOR: Enable/disable colors (default: "auto")
#   - ATOMIC_PROMPT_SHOW_REPO: Show repository name (default: "false")

# Cache variables
set -g _atomic_prompt_cache ""
set -g _atomic_prompt_cache_dir ""
set -g _atomic_prompt_cache_time 0

# Main prompt function
function atomic_prompt
    # Get current directory
    set -l current_dir $PWD

    # Simple cache: only update if directory changed or cache is old (5 seconds)
    set -l current_time (date +%s)
    if test "$current_dir" = "$_atomic_prompt_cache_dir" -a (math $current_time - $_atomic_prompt_cache_time) -lt 5
        echo -n "$_atomic_prompt_cache"
        return 0
    end

    # Build atomic prompt command arguments
    set -l atomic_args

    if set -q ATOMIC_PROMPT_FORMAT
        set -a atomic_args --format "$ATOMIC_PROMPT_FORMAT"
    end

    if test "$ATOMIC_PROMPT_SHOW_REPO" = "true"
        set -a atomic_args --show-repository
    end

    # Run atomic prompt command (silently fails if not in a repo)
    set -l atomic_output (atomic prompt $atomic_args 2>/dev/null)

    if test -n "$atomic_output"
        # Apply colors if enabled
        set -l color_choice "auto"
        if set -q ATOMIC_PROMPT_COLOR
            set color_choice $ATOMIC_PROMPT_COLOR
        end

        # Determine if colors should be used
        set -l use_colors false
        switch $color_choice
            case always
                set use_colors true
            case never
                set use_colors false
            case auto
                # Check if stdout is a terminal
                if isatty stdout
                    set use_colors true
                end
        end

        # Format output with or without colors
        if test "$use_colors" = "true"
            # Purple color for channel (matching bash version)
            set_color --bold brmagenta
            echo -n "$atomic_output"
            set_color normal
            echo -n " "
            set _atomic_prompt_cache (set_color --bold brmagenta)"$atomic_output"(set_color normal)" "
        else
            echo -n "$atomic_output "
            set _atomic_prompt_cache "$atomic_output "
        end

        # Update cache
        set -g _atomic_prompt_cache_dir "$current_dir"
        set -g _atomic_prompt_cache_time $current_time
    else
        # Not in an atomic repository - clear cache
        set -g _atomic_prompt_cache ""
        set -g _atomic_prompt_cache_dir ""
        set -g _atomic_prompt_cache_time 0
    end

    return 0
end

# Example fish_prompt function (commented out - user should add to their config)
#
# function fish_prompt
#     # Show atomic channel if in repo
#     atomic_prompt
#
#     # Show username@hostname
#     set_color green
#     echo -n (whoami)
#     set_color normal
#     echo -n '@'
#     set_color green
#     echo -n (hostname -s)
#     set_color normal
#     echo -n ':'
#
#     # Show current directory
#     set_color blue
#     echo -n (prompt_pwd)
#     set_color normal
#
#     # Show prompt character
#     echo -n '> '
# end

# Minimal example:
#
# function fish_prompt
#     atomic_prompt
#     set_color blue
#     echo -n (prompt_pwd)
#     set_color normal
#     echo -n '> '
# end

# Advanced example with git integration:
#
# function fish_prompt
#     # Show atomic channel if in atomic repo
#     atomic_prompt
#
#     # Show git branch if in git repo
#     if git rev-parse --git-dir > /dev/null 2>&1
#         set_color cyan
#         echo -n '(git:'
#         echo -n (git branch --show-current 2>/dev/null)
#         echo -n ') '
#         set_color normal
#     end
#
#     # Show current directory
#     set_color yellow
#     echo -n (prompt_pwd)
#     set_color normal
#
#     # Show status symbol
#     if test $status -eq 0
#         set_color green
#         echo -n ' ✓'
#     else
#         set_color red
#         echo -n ' ✗'
#     end
#     set_color normal
#     echo -n ' > '
# end
