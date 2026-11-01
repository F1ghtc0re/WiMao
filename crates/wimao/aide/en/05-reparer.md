# Repair

Over time, the Wine environment may go wrong: a component is missing, a plugin is no longer connected to Linux, Wine stays stuck. The **Repair** button looks for these problems and fixes them.

## When to use it?

- A plugin or a manager refuses to open.
- A WiMao error message offers **Repair**: one click is enough, only the problem concerned is fixed.
- At startup, WiMao reports that some plugins are not bridged yet, for example if WiMao was closed during a manager session. The message offers **Repair**.
- When in doubt, you can try **Repair**: your plugins are not modified, and WiMao normally creates a restore point first that you can go back to from the history.

## How to do it

1. Close your music software.
2. Click **Repair**, at the top right (the gear icon).
3. WiMao looks for problems. Repair stops Wine: if Windows programs are open, WiMao names them and asks **Close and repair**. **Cancel** gives up the whole repair, without changing anything.
4. WiMao creates a restore point, then fixes each problem found.

At the end, a message says **Repair finished** or **No problem found**.

## What WiMao can repair

- plugins that are installed but not bridged to Linux yet;
- Wine stuck: it is running although nothing uses it any more;
- folders of the environment (Documents, Desktop…) that lead to your personal folders: installers would fill them with their files;
- missing or broken Windows components: Wine Mono (for .NET programs), DXVK (for the display) and the Microsoft msvcp140 library;
- the download folder of Native Access.

Each repair also applies the settings of the installed managers again and restarts their services.

## What if the repair goes wrong?

The restore point created just before lets you go back: see [History and rollback](aide:historique). WiMao keeps the last three points created before a repair, separately from those of the installations.

If the problem comes back, use **Copy the report** in the main menu and attach it to your request for help. WiMao removes your user name, your e-mail addresses and the name of your computer from it. Nothing is sent: you paste the report yourself. See also [Common problems](aide:problemes).
