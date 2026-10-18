# Two panes, one safe copy

The captioned macOS clip in Procyon's Help panel shows a staged copy of the
public Procyon `README.md` in the **left pane** and `~/Downloads/test` in the
**right pane**. Downloads and Documents appear only as tab headers; their
contents are not shown. This edited demonstration is not an exercise: watching
it changes no files on your machine.

To try the same workflow with your own files:

1. Open a folder with a file you can use in the left pane. In the right pane,
   open a writable destination folder. Use the tab headers to return to other
   folders later; the highlighted pane is the active one.
2. Select a text file such as `README.md`. Press **F3** to view it, then close
   the viewer. Press **F4** to open it in the editor; close without saving if
   you are only exploring.
3. Select that file and choose **Pack to Archive** from the command palette
   or use its shortcut. Create a ZIP archive and wait for the operation to finish.
4. Select the new ZIP in the left pane. Confirm that the right pane displays
   the intended destination, then press **F5** to copy. Review the operation
   and any conflict prompt before proceeding. The source remains in place.

The example copies the resulting ZIP into `~/Downloads/test`; on your system,
choose any writable folder you control. Do not copy over an existing file
unless you intend to replace it. **F1** shows the current shortcut bindings.

A **workspace** saves your arrangement of panes, tabs, and view settings, not
copies of the files in those folders. See [Organize with workspaces](workspaces.md).
