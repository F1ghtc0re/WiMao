# History and rollback

WiMao records everything it does in a history: installations, manager sessions, repairs, backups, restores, imports and rollbacks. From there, you can go back to the state before an action.

## Opening the history

Click the clock-shaped button in the title bar, or press Ctrl+H.

The actions are grouped by day: **Today**, **Yesterday**, then the date. Each line shows what was done, on what, at what time, and the result: **Succeeded**, **Failed** or **Cancelled**. As long as no action has been done, the page shows "Nothing yet".

## Restore points

Before each installation, each manager session and each repair, WiMao creates a restore point: a snapshot of the settings of the Wine environment and of the list of its files.

WiMao keeps the last ten points of installations and sessions, and the last three points of repairs. Older ones are removed automatically.

## Going back to before an action

1. Close your music software.
2. In the history, find the action to undo and click **Go back to before this**.
3. WiMao shows the point used, its date and the number of files that will be removed. Confirm.

WiMao then puts the settings of the environment back as they were, and removes the files that appeared since: for example the plugins of a failed installation. Links to plugins that no longer exist are also removed from the plugin folders. The rollback in turn appears in the history.

Be careful: a rollback also undoes everything that was done **after** this point. Files modified since are not given back their old content: only the settings are.

## Why is the button missing or greyed out?

- The action did not create a restore point (a backup, for example).
- The point was removed because it was too old.
- A backup has been restored since: the older points no longer match the environment and are no longer offered.
- You went back to before an older action: the points created after it no longer match the environment and are no longer offered.
- The import was cancelled, or a new environment was created: the existing points describe another environment and are no longer offered.
- Another operation is in progress: wait until it is finished.

To put the whole environment back in a known state, use a backup instead: see [Backup and restore](aide:sauvegarde).
