# Adding a plugin

You downloaded the Windows installer of a plugin (an `.exe` or `.msi` file, or a `.zip` or `.7z` archive)? WiMao takes care of the rest.

## Starting the installation

1. Click **Add an installer** (or press Ctrl+O), then choose the file. You can also drop it on the window.
2. If a Windows program is still open (a manager, for example), WiMao names it and asks **Close and continue**: the installation has to stop Wine.
3. WiMao first creates a restore point, then runs the installer silently, without questions.
4. It then bridges the new plugins to Linux and tests that each one opens, in an invisible window.

At the end, a message sums up what was done, for example "2 plugins installed". The plugins appear in the list. A plugin reinstalled in a newer version carries the **Updated** mark until the window is closed.

If the plugin must be activated with your account (Serum 2, for example), an "Activate Serum 2" window explains what to do.

## When the silent installation is not enough

Some installers refuse to install silently. WiMao notices it: no plugin was added. It then offers **Open installer**. The usual window of the installer opens: follow it as on Windows, keeping the suggested folders. When it closes, WiMao bridges and tests the new plugins.

## The state of the plugins

- **Ready**: the plugin loads correctly in Wine. Depending on the case, WiMao saw its interface open, or could not check it: the explanation under its name says which ("Loads correctly; open it in your music software to check its interface"). Either way it can be used; open it in your music software to see its interface.
- **Check**: the test could not conclude, for example because the plugin did not finish loading within 5 minutes. Try it in your music software.
- **Problem**: the plugin does not load, or crashes in Wine. See [Common problems](aide:problemes).
- **Not tested**: the plugin has not been tested yet. CLAP plugins are not tested yet.

Under the name of each plugin, a short explanation goes with this state.

## Good to know

- If a plugin exists as VST3 and as VST2, WiMao keeps only the VST3: you will not get duplicates in your music software.
- 32-bit Windows plugins are not supported. Almost all recent plugins are 64-bit.
- Close your music software before installing: WiMao refuses to stop Wine while a plugin is in use.
- A failed installation? You can go back: see [History and rollback](aide:historique).
