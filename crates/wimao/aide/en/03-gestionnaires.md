# Publisher managers

Many publishers deliver their plugins through their own app: the manager. You sign in with your account, then it downloads, installs and activates your products. WiMao can install and open these four managers:

- **Native Access 2** (Native Instruments: Kontakt, Massive, Guitar Rig…);
- **Arturia Software Center** (Pigments, Analog Lab…);
- **inMusic Software Center** (AIR plugins: Hype, Velvet, Mini Grand…);
- **Plugin Alliance** (Installation Manager).

They are listed in the **Managers** section of the main window.

## Installing a manager

Click **Install** next to its name. WiMao downloads and installs it on its own. The button then becomes **Open**.

For Native Access, WiMao asks a **Sign-in link** question if another program already handles this link: answer **Use WiMao**. Otherwise, the sign-in return from your web browser would not reach Native Access.

## A manager session

1. Click **Open**. If other Windows programs are open, WiMao asks for your agreement to close them.
2. WiMao creates a restore point, then opens the manager. A banner says that it is open and that WiMao bridges the new plugins.
3. Sign in to your account. Depending on the manager, your web browser may open for the sign-in, then bring you back to the manager.
4. Install and activate your products as usual. Meanwhile, WiMao bridges each new plugin as soon as it appears.
5. When you are done, close the manager or click **I'm done** in the banner.

If an installer is still running, WiMao waits for it to finish. You can choose **Keep waiting** or **Stop anyway**. A message then sums up the session, for example "Native Access 2 closed: 3 plugins installed".

During a session, the other actions (install, repair, back up) are greyed out: their tooltip says why.

## Tips for each manager

- **Native Access**: it needs a download service and a download folder. WiMao installs and sets them up on its own.
- **Arturia Software Center**: a WebView2 error at startup has no consequence.
- **inMusic Software Center**: WiMao reruns its installers silently. If a product still shows "Install" after its installation, click it again or reopen the Software Center.
- **Plugin Alliance**: the Installation Manager may freeze at 98% on the last product. Wait until all the products are shown as installed, then close it. Closing it earlier loses the downloaded packages.

A manager does not open, or the sign-in does not come back? See [Common problems](aide:problemes).

## Other applications

Some installers added with **Add an installer** install no plugin: they put down the publisher's launcher (for example BLEASS Plugins Installer). WiMao notices the new Start-menu shortcut and puts the launcher in the **Managers** section, under the known managers. Its row reads "Added application · Installed".

- **Open** starts a session exactly like a manager: restore point, plugins bridged as they appear, then **I'm done**.
- The "⋯" menu offers **Remove from the list**, after a confirmation. It only removes the row: nothing is uninstalled.

When the installation ends, WiMao shows "{name} installed: open it from the Managers section".

Limit: a launcher without a Start-menu shortcut is not detected.

On the command line: `wimao-cli application lister`, `wimao-cli application ouvrir <name>` and `wimao-cli application retirer <name>`.
