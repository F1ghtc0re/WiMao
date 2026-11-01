# Importing an existing environment

Before WiMao, your plugins may already have been installed by hand, in a Wine environment kept in the `~/.wine-mao` folder. Rather than reinstalling everything, WiMao can take it over as it is.

## When is the import offered?

At the first launch, when WiMao does not have an environment of its own yet. If it finds a valid `~/.wine-mao`, the assistant shows its size and its number of plugins, then offers:

- **Import this environment (recommended)**: WiMao takes over your plugins, their settings and their licences;
- **Create a new environment**: WiMao starts from scratch and leaves `~/.wine-mao` untouched.

## What the import does

1. WiMao checks that no plugin is running: close your music software before you start. If a Windows program is open, it names it and asks for your agreement before closing it.
2. It sets aside the settings of the environment and those of yabridge: a small backup, just in case.
3. It moves the folder into its own data folder. The move is instant, even for more than 100 GB: nothing is copied.
4. At the old place, it leaves a link to the new location. The plugins already bridged therefore keep working: your music software sees no difference.

The import then appears in the history. Your personal yabridge settings (`~/.config/yabridgectl`) are never changed.

## Import refused: two different disks

The instant move is only possible within the same disk. If `~/.wine-mao` and the WiMao data folder are on two different disks, WiMao refuses the import and explains why. It never makes a copy: the environment is too large. Nothing is changed: your old environment still works where it is. You can also move the folder yourself, then start WiMao again.

## Cancelling the import

As long as it is possible, the main menu offers **Cancel the import**. WiMao then removes the link, puts the folder back in its original place in `~/.wine-mao` and updates the links of your plugins with your own yabridge configuration. Nothing is deleted.

The cancellation is refused if `~/.wine-mao` is no longer the link created by the import: WiMao prefers not to overwrite anything. Like the import, it requires your music software to be closed.

After the cancellation, WiMao has no environment any more: the first-launch assistant comes back. The restore points created until then are no longer offered in the history: they describe the environment returned to `~/.wine-mao`. See [Getting started](aide:premiers-pas) and [Common problems](aide:problemes).
