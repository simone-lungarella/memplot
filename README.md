# memplot

![Linux](https://img.shields.io/badge/os-linux-blue)
![macOS](https://img.shields.io/badge/os-macos-lightgrey)

Plot real-time memory usage (RSS) of a process directly in your terminal.

`memplot` is a lightweight TUI (terminal UI) tool built with [ratatui](https://github.com/ratatui/ratatui) and `crossterm`.

It continuously samples a process' resident set size and renders a live scrolling chart with current, min, and max values.

> ⚠️ **Platform support**  
> Currently works on **Linux and macOS only**;
> On Linux it reads from `/proc/<pid>/statm`. macOS support depends on compatible system APIs;

## Usage

```bash
memplot <pid>
```

## Example

![Example](./showcase.png)

## Notes

- On Linux, RSS is obtained via `/proc/<pid>/statm`
- The tool exits automatically if the process disappears
- Designed to stay dependency-light and terminal-friendly
