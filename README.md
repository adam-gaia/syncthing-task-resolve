# syncthing-task-resolve

Resolve conflicts from syncing taskwarrior (taskchampion.sqlite3) databases with syncthing

## What is this crate for?

### TL;DR

With two iOS apps ($5 each, not open source), and this rust crate (free and open source), one can sync taskwarrior tasks between linux machines and iPhone/iPads.

### The Problem

I use [taskwarrior](https://taskwarrior.org/) for managing my todo lists on my **linux** laptop.

I also have an iPhone. It's probably obvious, but getting my taskwarrior tasks on my iPhone is not easy;
Either I should get an android phone or develop my own IOS taskwarrior app.
Since both would be a big effort and its not clear which will happen first, if at all, I've elected for a band-aid solution.

### The Band-aid Solution

I have two apps that make working with taskwarrior tasks on IOS possible.

The first is [Taskchamp](https://github.com/marriagav/taskchamp-docs). Not to be confused with [taskchampion](https://github.com/GothenburgBitFactory/taskchampion), taskwarrior v3's sync backend.
Taskchamp's main is that one can easily sync the taskwarrior database between an iPhone and a Mac via iCloud.
I'm sure that all works great, but I just sold my macbook a few months ago in favor of running linux full time on a framework laptop (go get 'em *champ*!).

For syncing iOS \<-> linux, I use [Mobius Sync](https://mobiussync.com/), a [syncthing](https://syncthing.net/) client for iOS.
This app (the $5 paid version) has the ability to sync files in other apps file sandboxes,
which many (well probably at least a dozen lol) people use to sync their \[Obsidan\] vaults to iOS devices without Obsidian's monthly subscription.
I checked it out for that use case, and figured it would work with syncing taskwarrior data as well.

Unfortunately, neither app is fully open source.

Syncing this way worked pretty well out of the box... with one pretty major issue.

#### Problems caused by the solution

The version of the database on my linux machine constantly overrides the version on my iPhone.
I believe this must be because Taskwarrior *modifies* the database each time it is accessed. I haven't looked into the internals of taskwarrior, but
this is consistent with [Syncthing's docs on conflicting changes](https://docs.syncthing.net/users/syncing.html#conflicting-changes).
When a file has been modified on two devices, syncthing takes the one with the latest modified time and calls the other a 'conflict'.

I've tested this and found that I lose tasks created on my iPhone when I simply run `task list` on my laptop before syncthing has had a chance to sync.
If I don't touch taskwarrior on the laptop for a few minutes (while syncthing syncs), the new tasks created on the iPhone do show up.

#### Solutions to problems caused by the original solution

Fortunately, syncthing saves the discarded version of the file conflict. These files are saved next to the override as `taskchampion.sync-conflict-<timestamp>-<originating device>.sqlite3`.
I've written a simple program (this repo's rust crate) to merge the taskwarrior database syncthing conflicts.
By running this tool periodically, conflicts are resolved and sync behavior works as expected.

## Disclaimer

I've been using this setup for a while now and haven't had any problems. But of course, this is a band-aid for a hacky, unsupported process.
Yada yada yada, use at your own risk!

## Usage

- Standalone

```console
$ syncthing-task-resolve
```

- As a taskwarrior hook
  TODO: write up how to set up to run whenever we run 'task'

- With a file watcher
  TODO: add notes on running whenever a syncthing conflict file is created

## Configuration

TODO: add notes on config
