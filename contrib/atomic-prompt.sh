#!/bin/bash
# Atomic VCS Prompt Integration
#
# This script provides shell prompt integration for Atomic VCS,
# displaying the current channel in your terminal prompt.
#
# Installation:
#   1. Source this file in your ~/.bashrc or ~/.zshrc:
#      source /path/to/atomic-prompt.sh
#
#   2. Add the atomic_prompt function to your PS1:
#      PS1='$(atomic_prompt)\w\$ '
#
# Configuration:
#   Set these environment variables to customize the prompt:
#   - ATOMIC_PROMPT_FORMAT: Custom format string (default: "[{channel}]")
#   - ATOMIC_PROMPT_COLOR: Enable/disable colors (default: "auto")
#   - ATOMIC_PROMPT_SHOW_REPO: Show repository name (default: "false")

# Cache for performance
_ATOMIC_PROMPT_CACHE=""
_ATOMIC_PROMPT_CACHE_DIR=""
_ATOMIC_PROMPT_CACHE_TIME=0

# Color codes (for printf)
_ATOMIC_COLOR_RESET='\033[0m'
_ATOMIC_COLOR_CHANNEL='\033[38;5;141m'  # Purple
_ATOMIC_COLOR_REPO='\033[38;5;244m'     # Gray

# Determine if colors should be used
_atomic_use_colors() {
    local color_choice="${ATOMIC_PROMPT_COLOR:-auto}"

    case "$color_choice" in
        always) return 0 ;;
        never) return 1 ;;
        auto)
            # Check if stdout is a terminal and supports colors
            if [ -t 1 ] && [ "${TERM:-}" != "dumb" ]; then
                return 0
            fi
            return 1
            ;;
    esac
}

# Main prompt function
atomic_prompt() {
    # Check if we're in a git repository to avoid confusion
    # (Users might be transitioning from git to atomic)
    local current_dir="$PWD"

    # Simple cache: only update if directory changed or cache is old (5 seconds)
    local current_time=$(date +%s)
    if [ "$current_dir" = "$_ATOMIC_PROMPT_CACHE_DIR" ] && \
       [ $((current_time - _ATOMIC_PROMPT_CACHE_TIME)) -lt 5 ]; then
        # Return cached result (without color codes in cache for simplicity)
        if _atomic_use_colors; then
            printf "${_ATOMIC_COLOR_CHANNEL}%s${_ATOMIC_COLOR_RESET}" "$_ATOMIC_PROMPT_CACHE"
        else
            printf "%s" "$_ATOMIC_PROMPT_CACHE"
        fi
        return 0
    fi

    # Try to get atomic prompt
    local atomic_output
    local format_flag=""

    if [ -n "$ATOMIC_PROMPT_FORMAT" ]; then
        format_flag="--format '$ATOMIC_PROMPT_FORMAT'"
    fi

    local show_repo_flag=""
    if [ "$ATOMIC_PROMPT_SHOW_REPO" = "true" ]; then
        show_repo_flag="--show-repository"
    fi

    # Run atomic prompt command (silently fails if not in a repo)
    atomic_output=$(atomic prompt $show_repo_flag $format_flag 2>/dev/null)

    if [ -n "$atomic_output" ]; then
        # Apply colors if enabled
        if _atomic_use_colors; then
            # Add color to the output - store for cache
            _ATOMIC_PROMPT_CACHE="${atomic_output} "
            # Use printf to interpret escape sequences
            printf "${_ATOMIC_COLOR_CHANNEL}%s${_ATOMIC_COLOR_RESET} " "$atomic_output"
        else
            _ATOMIC_PROMPT_CACHE="${atomic_output} "
            printf "%s" "$_ATOMIC_PROMPT_CACHE"
        fi

        # Update cache
        _ATOMIC_PROMPT_CACHE_DIR="$current_dir"
        _ATOMIC_PROMPT_CACHE_TIME=$current_time
    else
        # Not in an atomic repository - clear cache
        _ATOMIC_PROMPT_CACHE=""
        _ATOMIC_PROMPT_CACHE_DIR=""
        _ATOMIC_PROMPT_CACHE_TIME=0
    fi

    return 0
}

# For zsh compatibility
if [ -n "$ZSH_VERSION" ]; then
    # Enable prompt substitution in zsh
    setopt PROMPT_SUBST
fi

# Example PS1 configurations (commented out - user should add to their shell config)
#
# For Bash:
# PS1='$(atomic_prompt)\[\033[01;32m\]\u@\h\[\033[00m\]:\[\033[01;34m\]\w\[\033[00m\]\$ '
#
# For Zsh:
# PROMPT='$(atomic_prompt)%F{green}%n@%m%f:%F{blue}%~%f%# '
#
# Minimal:
# PS1='$(atomic_prompt)\w\$ '

# Advanced example with error code display
# PS1='$(atomic_prompt)$(if [ $? -eq 0 ]; then echo "\[\033[0;32m\]✓"; else echo "\[\033[0;31m\]✗"; fi)\[\033[0m\] \w\$ '

# Git-style example (shows branch/channel)
# PS1='$(atomic_prompt)\[\033[00;33m\]\w\[\033[00m\]$(if git rev-parse --git-dir > /dev/null 2>&1; then echo " \[\033[00;36m\](git:$(git branch --show-current))\[\033[00m\]"; fi)\$ '
