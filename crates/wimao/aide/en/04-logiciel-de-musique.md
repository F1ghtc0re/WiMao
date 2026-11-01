# Using plugins in your music software

WiMao puts the bridged plugins in three folders of your home folder, one per format:

- `~/.vst3/yabridge` for VST3 plugins;
- `~/.vst/yabridge` for VST2 plugins;
- `~/.clap/yabridge` for CLAP plugins.

The `~` sign stands for your home folder. These folders start with a dot: they are hidden. In a file chooser, show the hidden folders (often with Ctrl+H) or type the path directly.

These are ordinary Linux plugins: any Linux music software that loads these formats can use them (Bitwig, Ardour, REAPER, Renoise, Carla, Qtractor…). So far, only Bitwig has been tested with WiMao.

## A setting to make only once

Most programs already scan `~/.vst3` and `~/.clap`, subfolders included: your VST3 and CLAP plugins are therefore often found by themselves. VST2 has no standard location: you usually have to add `~/.vst/yabridge` by hand. When in doubt, add the three folders.

### Bitwig

1. Open Bitwig, then its settings: **Settings → Locations**.
2. In the plug-in locations, add the three folders above.
3. Click **Rescan** so that Bitwig reads the folders again.

### Ardour

In the preferences, Plugins section, add the VST folders above if needed, then start the plugin scan from the Plugin Manager.

### Other programs

REAPER, Renoise…: add the same folders in their plugin settings, then run the plugin scan again.

Your Windows plugins then appear in the plugin browser, next to the Linux plugins. You use them exactly like the others.

## After each addition

There is nothing to redo in the settings. After an installation or a manager session, simply run the plugin scan again in your software (or restart it) to see the new plugins.

## Closing your music software before some actions

When a Windows plugin is open in your software, Wine works for it. So WiMao refuses to stop Wine while a plugin is running: otherwise, the software would lose the plugin in the middle of a session. Close it before you:

- install a plugin or open a manager;
- repair;
- back up or restore;
- go back from the history;
- import an environment.

If you forget, WiMao shows "Close your music software before continuing: a plugin is running". Nothing is changed: close your software and start again.

## Good to know

- The first loading of a Windows plugin may take a few seconds longer than that of a Linux plugin.
- A plugin marked **Problem** in WiMao may sometimes work in your software: the WiMao test uses another host program. Try it before you worry.
- After a rollback, the removed plugins disappear from these folders. Run the plugin scan again so that your software forgets them.

A plugin refuses to open? See [Common problems](aide:problemes).
