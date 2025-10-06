# Atomic VCS Prompt Integration

Display the current Atomic channel in your terminal prompt, similar to how Git shows the current branch.

## Quick Start

### Bash/Zsh

Add to your `~/.bashrc` or `~/.zshrc`:

```bash
source /path/to/atomic/contrib/atomic-prompt.sh
PS1='$(atomic_prompt)\w\$ '
```

### Fish

Add to your `~/.config/fish/config.fish`:

```fish
source /path/to/atomic/contrib/atomic-prompt.fish

function fish_prompt
    atomic_prompt
    set_color blue
    echo -n (prompt_pwd)
    set_color normal
    echo -n '> '
end
```

## Features

- **Automatic Detection**: Only shows when inside an Atomic repository
- **Fast Performance**: Caches results for 5 seconds to avoid repeated calls
- **Color Support**: Automatically detects terminal color capabilities
- **Configurable Format**: Customize how the channel is displayed
- **Repository Name**: Optionally show the repository name
- **Shell Agnostic**: Works with Bash, Zsh, and Fish shells

## Configuration

### Environment Variables

Configure the prompt behavior using environment variables:

```bash
# Custom format string (default: "[{channel}]")
export ATOMIC_PROMPT_FORMAT="[{repository}:{channel}]"

# Color output: auto, always, never (default: auto)
export ATOMIC_PROMPT_COLOR="always"

# Show repository name (default: false)
export ATOMIC_PROMPT_SHOW_REPO="true"
```

### Configuration File

Alternatively, configure in `~/.config/atomic/config.toml`:

```toml
[prompt]
enabled = true
format = "[{channel}]"
show_repository = false
```

### Format Placeholders

Available placeholders in the format string:

- `{channel}` - Current channel name (e.g., "main", "feature-branch")
- `{repository}` - Repository name (e.g., "my-project")

## Command-Line Usage

The `atomic prompt` command can also be used directly:

```bash
# Basic usage (uses config defaults)
atomic prompt

# Custom format
atomic prompt --format "{channel}"

# Show only channel name
atomic prompt --channel-only

# Include repository name
atomic prompt --show-repository

# Custom format with repository
atomic prompt --format "[{repository}:{channel}]" --show-repository
```

## Example Prompts

### Minimal (Bash/Zsh)

```bash
PS1='$(atomic_prompt)\w\$ '
# Output: [main] ~/code/project $
```

### Full Featured (Bash)

```bash
PS1='$(atomic_prompt)\[\033[01;32m\]\u@\h\[\033[00m\]:\[\033[01;34m\]\w\[\033[00m\]\$ '
# Output: [main] user@host:~/code/project $
```

### With Status (Bash)

```bash
PS1='$(atomic_prompt)$(if [ $? -eq 0 ]; then echo "\[\033[0;32m\]✓"; else echo "\[\033[0;31m\]✗"; fi)\[\033[0m\] \w\$ '
# Output: [main] ✓ ~/code/project $
```

### Git and Atomic (Bash)

```bash
git_prompt() {
    if git rev-parse --git-dir > /dev/null 2>&1; then
        echo "(git:$(git branch --show-current)) "
    fi
}

PS1='$(atomic_prompt)$(git_prompt)\w\$ '
# Output: [main] (git:feature-branch) ~/code/project $
```

### Fish Advanced

```fish
function fish_prompt
    # Atomic channel
    atomic_prompt

    # Git branch (if in git repo)
    if git rev-parse --git-dir > /dev/null 2>&1
        set_color cyan
        echo -n '(git:'(git branch --show-current)')'
        set_color normal
        echo -n ' '
    end

    # Current directory
    set_color yellow
    echo -n (prompt_pwd)
    set_color normal

    # Status indicator
    if test $status -eq 0
        set_color green
        echo -n ' ✓'
    else
        set_color red
        echo -n ' ✗'
    end
    set_color normal
    echo -n ' > '
end
# Output: [main] ~/code/project ✓ >
```

## Performance Considerations

The prompt integration is optimized for performance:

1. **Fast Command**: The `atomic prompt` command is lightweight and exits quickly
2. **Caching**: Results are cached for 5 seconds to avoid repeated repository lookups
3. **Silent Failure**: When not in a repository, the command exits immediately with no output
4. **No Blocking**: The command never blocks or waits for user input

### Benchmarks

Typical execution times:

- **Inside repository**: ~5-15ms (first call), ~0.1ms (cached)
- **Outside repository**: ~1-3ms

## Troubleshooting

### Prompt not showing

1. **Check if in an Atomic repository**:
   ```bash
   atomic channel
   ```

2. **Test the command directly**:
   ```bash
   atomic prompt
   ```

3. **Verify configuration**:
   ```bash
   cat ~/.config/atomic/config.toml
   ```

### Slow prompt

1. **Check cache timeout**: The cache expires after 5 seconds. Frequent directory changes will trigger new lookups.

2. **Disable if needed**: Set `enabled = false` in config or unset the prompt function.

### Colors not working

1. **Check terminal support**:
   ```bash
   echo $TERM
   ```

2. **Force colors**:
   ```bash
   export ATOMIC_PROMPT_COLOR="always"
   ```

3. **Disable colors**:
   ```bash
   export ATOMIC_PROMPT_COLOR="never"
   ```

## Integration with Prompt Frameworks

### Oh My Zsh

Create a custom theme in `~/.oh-my-zsh/custom/themes/atomic.zsh-theme`:

```zsh
source /path/to/atomic/contrib/atomic-prompt.sh

PROMPT='$(atomic_prompt)%{$fg[cyan]%}%c%{$reset_color%} $(git_prompt_info)%# '

ZSH_THEME_GIT_PROMPT_PREFIX="%{$fg[blue]%}("
ZSH_THEME_GIT_PROMPT_SUFFIX="%{$reset_color%} "
ZSH_THEME_GIT_PROMPT_DIRTY="%{$fg[blue]%}) %{$fg[yellow]%}✗"
ZSH_THEME_GIT_PROMPT_CLEAN="%{$fg[blue]%})"
```

### Starship

Add to `~/.config/starship.toml`:

```toml
[custom.atomic]
command = "atomic prompt --channel-only"
when = "test -d .atomic"
format = "[$output]($style) "
style = "bold purple"
```

### Powerlevel10k

Add to `~/.p10k.zsh`:

```zsh
function prompt_atomic() {
    local channel=$(atomic prompt --channel-only 2>/dev/null)
    if [[ -n "$channel" ]]; then
        p10k segment -f 141 -t "$channel"
    fi
}

typeset -g POWERLEVEL9K_RIGHT_PROMPT_ELEMENTS=(
    atomic
    # ... other elements
)
```

## Advanced Usage

### Conditional Formatting

Different colors based on channel name (Bash):

```bash
atomic_prompt_colored() {
    local channel=$(atomic prompt --channel-only)
    if [ -n "$channel" ]; then
        case "$channel" in
            main|master)
                echo -ne "\[\033[38;5;46m\][$channel]\[\033[0m\] "  # Green
                ;;
            develop|dev)
                echo -ne "\[\033[38;5;226m\][$channel]\[\033[0m\] " # Yellow
                ;;
            *)
                echo -ne "\[\033[38;5;141m\][$channel]\[\033[0m\] " # Purple
                ;;
        esac
    fi
}

PS1='$(atomic_prompt_colored)\w\$ '
```

### Show Change Count

Display number of unrecorded changes (requires additional work):

```bash
atomic_prompt_with_status() {
    local prompt=$(atomic prompt)
    if [ -n "$prompt" ]; then
        # This is conceptual - actual implementation would need atomic support
        local changes=$(atomic diff --summary 2>/dev/null | wc -l)
        if [ "$changes" -gt 0 ]; then
            echo -ne "$prompt\[\033[0;33m\]+$changes\[\033[0m\] "
        else
            echo -ne "$prompt"
        fi
    fi
}
```

## Comparison with Git Prompt

| Feature | Atomic Prompt | Git Prompt |
|---------|---------------|------------|
| Show current branch/channel | ✓ | ✓ |
| Color support | ✓ | ✓ |
| Performance caching | ✓ | Varies |
| Repository detection | ✓ | ✓ |
| Dirty state indicator | ⚠️ (future) | ✓ |
| Upstream tracking | ⚠️ (future) | ✓ |

## Contributing

To improve the prompt integration:

1. Test on different shells (Bash, Zsh, Fish)
2. Benchmark performance in large repositories
3. Add support for additional shells (PowerShell, Nushell)
4. Implement status indicators (dirty working copy, ahead/behind)

## See Also

- [Atomic VCS Documentation](../docs/)
- [Atomic Configuration Guide](../docs/configuration.md)
- [Shell Completion](./completions/)

---

**Questions or Issues?** Open an issue on the Atomic VCS repository.