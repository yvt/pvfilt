# pvfilt

Reads a program's output and extracts numbers to display a chart, estimated time to complete, and a progress bar.

![](https://yvt.jp/files/programs/pvfilt/pvfilt-2019-11-20.png)

## Usage

(1) **Watch mode** — Executes a given command periodically like watch(1).

    pvfilt -w -- dmsetup status

(2) **Run-once mode** — Executes a given command and processes each outputted line (**WIP**).

    # does not work yet!
    # pvfilt -- ninja

(3) **Pipe mode** — Like the previous mode, but instead reads from stdin (**WIP**).

    # does not work yet!
    # commandname | pvfilt

## Unimplemented Features

- Customizing the value detection. Currently the pattern is hard-coded as `[0-9]+/[0-9]+`
    - Profiles (Automatically choose a regex based on the given command name)
- Multiple values
- Output scrolling
- Run-once mode
- Pipe mode
