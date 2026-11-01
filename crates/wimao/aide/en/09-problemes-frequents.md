# Common problems

## A plugin is marked "Problem"

Its opening test failed. Read the explanation under its name, then:

1. Try it in your music software anyway: the WiMao test uses another host program, which sometimes crashes where your music software works.
2. Click **Repair**: a missing component is often the cause.
3. If the plugin comes from a manager, check that it is properly installed and activated there.

A plugin marked **Check** loaded, but its interface could not be checked: try it in your music software.

## "A plugin is running"

The full message is "Close your music software before continuing: a plugin is running". WiMao refuses to stop Wine while a plugin is working in your music software, so as not to make it crash. Nothing was changed: close your music software, then start again.

## A manager does not open

- Wait a little: the first start of a manager may be slow.
- If a window stays black or empty, close it, then click **Repair**: the repair stops Wine, applies the managers' settings again and restarts their services. Then open the manager again.
- The **Open** button is greyed out? Another operation or a manager session is in progress: wait until it is finished.

## The web browser does not come back to the manager after signing in

For Native Access, the sign-in return goes through a `native-access://` link that WiMao passes on to the manager.

- Leave the manager open during the sign-in, and accept opening the link if the web browser asks.
- If WiMao shows "A sign-in link could not reach the manager", open the link again from the web browser.
- If you answered **Keep the current one** to the **Sign-in link** question, the link goes to another program. You can try adding the Native Access installer again with **Add an installer**: WiMao then asks the question again and you answer **Use WiMao**. There is no guarantee that this is enough.
- For a plugin that is activated through the web browser (Serum 2, for example), follow the instruction shown during its installation.

## Disk full during a backup

WiMao checks the free space before it starts and says how much is needed and how much is available. If the disk fills up or is unplugged while writing, WiMao deletes the pieces already written: your environment is not touched. Free some space or choose another disk, then start again. See [Backup and restore](aide:sauvegarde).

## The import is refused: two different disks

WiMao moves the environment only within the same disk, and never copies it: it is too large. Nothing was changed, and your old environment still works. See [Importing an existing environment](aide:import).

## Still stuck?

Choose **Copy the report** in the main menu, then paste it into your request for help. An installation that broke everything can also be undone: see [History and rollback](aide:historique).
