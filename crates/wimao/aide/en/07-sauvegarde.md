# Backup and restore

A backup copies the whole Wine environment into a single archive: everything installed in that environment, so your plugins, their settings, their licences and any sounds they placed there. Anything stored elsewhere on your disks is not part of it. If your disk fails or an update breaks everything, you get your installation back without downloading everything again.

## Making a backup

1. Close your music software and plug in an external disk.
2. Click **Back up**, at the top right (the disk icon).
3. Choose the destination folder, preferably on the external disk.
4. Wait until the end: it takes several minutes, depending on the size of your plugins and their sounds.

WiMao first checks that there is enough free space. It puts the backup in a `WiMao-sauvegarde` folder, as a dated archive, for example `wimao-2026-11-01-14h30.tar.zst`, together with a `.sha256` file. This file contains the checksums used to check that the archive is intact. Always keep both together.

At the end, a message says where the backup was saved. The **Show** button opens the folder.

## Good to know

- On a disk formatted as FAT32, which does not accept files larger than 4 GB, the archive is split into pieces of 3.9 GB (`.part000`, `.part001`…). Keep all the pieces.
- Three folders of each Windows user in the environment are not backed up: the temporary folder, the **Downloads** folder (downloaded installers) and the DXVK cache. They are not needed or are recreated on their own.
- If writing fails (disk unplugged or full), WiMao deletes the pieces already written. It never leaves an incomplete backup that would look complete.

## Restoring a backup

1. Close your music software and plug in the disk that holds the backup.
2. In the main menu, choose **Restore a backup…**.
3. Select the `.sha256` file of the backup.
4. WiMao shows the date and the size of the archive. Confirm.

WiMao first checks the checksums of all the pieces. If one is wrong, it stops without touching anything. Otherwise, it extracts the archive next to the current environment, then swaps the two. Your current environment is not deleted: it is kept under the name `prefixe.ancien-<date>`, in the WiMao data folder. You can delete it yourself once you are reassured.

At the end, a message confirms **Backup restored** and the list of plugins is read again.

After a restore, the old restore points no longer match the environment: the history no longer offers them. See [History and rollback](aide:historique).

A space problem? See [Common problems](aide:problemes).
