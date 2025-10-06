# Atomic VCS Prompt Integration - Quick Start

A one-page guide to get the current channel displayed in your terminal prompt.

## 1-Minute Setup

### Bash/Zsh

Add to `~/.bashrc` or `~/.zshrc`:

```bash
source /path/to/atomic/contrib/atomic-prompt.sh
PS1='$(atomic_prompt)\w\$ '
```

### Fish

Add to `~/.config/fish/config.fish`:

```fish
source /path/to/atomic/contrib/atomic-prompt.fish

function fish_prompt
    atomic_prompt
    echo -n (prompt_pwd)' > '
end
```

## Common Formats

```bash
# Default
atomic prompt
# Output: [main]

# Channel only (no brackets)
atomic prompt --channel-only
# Output: main

# Custom brackets
atomic prompt --format "({channel})"
# Output: (main)

# With repository name
atomic prompt --show-repository --format "[{repository}:{channel}]"
# Output: [my-project:main]

# Custom symbol
atomic prompt --format "⚛ {channel}"
# Output: ⚛ main
```

## Configuration

Create `~/.config/atomic/config.toml`:

```toml
[prompt]
enabled = true
format = "[{channel}]"
show_repository = false
```

Or use environment variables:

```bash
export ATOMIC_PROMPT_FORMAT="[{channel}]"
export ATOMIC_PROMPT_COLOR="auto"      # auto, always, never
export ATOMIC_PROMPT_SHOW_REPO="false" # true, false
```

## Full Prompt Examples

### Minimal
```bash
PS1='$(atomic_prompt)\w\$ '
# Output: [main] ~/code/project $
```

### With User and Host
```bash
PS1='$(atomic_prompt)\u@\h:\w\$ '
# Output: [main] user@host:~/code/project $
```

### With Git Integration
```bash
git_prompt() {
    if git rev-parse --git-dir > /dev/null 2>&1; then
        echo "(git:$(git branch --show-current)) "
    fi
}
PS1='$(atomic_prompt)$(git_prompt)\w\$ '
# Output: [main] (git:feature) ~/code/project $
```

### Colorful (Bash)
```bash
PS1='$(atomic_prompt)\[\033[01;32m\]\u@\h\[\033[00m\]:\[\033[01;34m\]\w\[\033[00m\]\$ '
# Output: [main] user@host:~/code/project $ (with colors)
```

### Fish Advanced
```fish
function fish_prompt
    atomic_prompt
    set_color green
    echo -n (whoami)'@'(hostname -s)
    set_color normal
    echo -n ':'
    set_color blue
    echo -n (prompt_pwd)
    set_color normal
    echo -n ' > '
end
# Output: [main] user@host:~/code/project >
```

## Performance

- First call: ~5-15ms
- Cached calls: <1ms
- Cache duration: 5 seconds
- Silently exits if not in a repository

## Troubleshooting

### No output when in repository?
```bash
# Check if you're in an Atomic repository
atomic channel

# Test the command directly
atomic prompt

# Check configuration
cat ~/.config/atomic/config.toml
```

### Prompt is slow?
The prompt caches results for 5 seconds. If you're frequently changing directories, you'll trigger new lookups. This is normal and optimized.

### Colors not working?
```bash
# Force colors on
export ATOMIC_PROMPT_COLOR="always"

# Or disable colors
export ATOMIC_PROMPT_COLOR="never"
```

## Demo

Run the interactive demo to see all features:

```bash
bash /path/to/atomic/contrib/prompt-demo.sh
```

## Learn More

- Full documentation: `contrib/PROMPT_INTEGRATION.md`
- Bash/Zsh script: `contrib/atomic-prompt.sh`
- Fish script: `contrib/atomic-prompt.fish`

---

**Quick Reference Card** | Atomic VCS | [Channel] in your prompt!