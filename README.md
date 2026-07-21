# dotshell

A command-line tool to start a shell with environment variables defined through .env files.

![GitHub Release](https://img.shields.io/github/v/release/fnando/dotshell)

## Install

### Homebrew

```console
$ brew install fnando/tap/dotshell
```

### Others

Download the binary for your architecture from
https://github.com/fnando/dotshell/releases/latest

## Usage

Load the init script on your rc file (e.g. `~/.zshrc`):

```bash
eval "$(dotshell shell-init)"
```

Use dotshell:

```console
$ dotshell
```
