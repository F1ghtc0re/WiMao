# Getting started

WiMao runs your Windows audio plugins (VST3, VST2, CLAP) on Linux, in your music software (DAW): Bitwig, Ardour, REAPER… You have nothing to set up by hand: WiMao installs your plugins, connects them to Linux and checks that they open.

## What is the Wine environment?

Wine runs Windows programs on Linux. To do so, it keeps everything in a folder that imitates a Windows computer: a `C:` drive, its programs and its settings. WiMao calls this folder the **Wine environment**.

Your Windows plugins and the publishers' apps (Native Access, Arturia Software Center…) are installed in this environment. WiMao creates it, looks after it and backs it up for you: you never need to open it.

For your music software to see these plugins, WiMao "bridges" them: for each one, it creates a small Linux plugin that connects to the Windows plugin.

## The first launch

At the first launch, an assistant welcomes you. Click **Start**, then:

- if it finds no environment, it offers **Create the environment**;
- if you already used an environment in `~/.wine-mao`, it offers **Import this environment (recommended)** or **Create a new environment**. See [Importing an existing environment](aide:import).

Once the environment is ready, choose **Add an installer**, **Open a publisher manager** or **Later**.

## The main window

- The list of your plugins, with the state of each one: **Ready**, **Check**, **Problem** or **Not tested**.
- The **Managers** section: the publishers' apps, to install or to open.
- At the top left, **Add an installer**. At the top right: the history, **Repair**, **Back up** and the main menu.

The main menu also lets you change the **Language** and open this **User guide** (F1 key).

## A good start

1. Add your plugins: [Adding a plugin](aide:ajouter-un-plugin) or [Publisher managers](aide:gestionnaires).
2. Tell your music software once where to find them: [Using plugins in your music software](aide:logiciel).
3. Make a backup on an external disk from time to time: [Backup and restore](aide:sauvegarde).
