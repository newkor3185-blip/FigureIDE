# FigureIDE desktop updates

FigureIDE's desktop updater checks only the public GitHub Releases API for
`newkor3185-blip/FigureIDE`. It does not use VectorCraft's release feed. Startup
checks run in a background thread, are enabled by default, and are limited to
one successful check per 24 hours. Help → Check for Updates always makes a new
request. Update preferences are saved with the existing FigureIDE UI settings.

The default channel is **Beta**. Beta accepts newer beta and stable semantic
versions; Stable ignores prereleases. Draft releases, malformed tags, other tag
prefixes, and versions that are not newer than the installed version are
ignored. GitHub's prerelease flag does not override the version encoded in a
`figureide-v…` tag.

Before offering a download, FigureIDE selects the exact release asset for the
current target, reads its exact entry in `SHA256SUMS`, downloads over HTTPS,
checks the SHA-256 value, then validates archive paths, links, expansion size,
package metadata and required license files. A matching checksum ties the
archive to the published checksum file; it does not independently authenticate
the release publisher.

## Installation support

- **Windows x64:** Automatic replacement is available for the portable ZIP
  distribution only. A separate FigureIDE helper waits for the app to exit,
  stages and validates the archive, keeps a backup during replacement, and
  relaunches the app. Installer or package-manager installations use the release
  page for manual updates.
- **Linux x86_64:** Automatic replacement is available only for the portable
  `FigureIDE-Linux-x86_64` directory when its parent is writable. The helper
  stages a copy beside the current package, waits for FigureIDE to exit, swaps
  directories and keeps a backup until the new app reaches its first UI frame.
  It does not use sudo and refuses symlinks, protected directories and unknown
  installations. Keep projects outside the application directory; configuration
  and projects are not removed by the update process.
- **macOS Apple Silicon and Intel:** The updater selects and verifies the
  architecture-specific ZIP, then offers the official release page for manual
  installation. Current beta packages may be unsigned and unnotarized. FigureIDE
  does not bypass Gatekeeper, change quarantine attributes or replace an app
  bundle automatically. Follow the release page's installation instructions
  and macOS security prompts.

No update is installed silently. The user must choose **Download and Verify**
and, where automatic replacement is supported, **Install and Restart**. Save
documents and finish R, render or export operations before installing. If a
package cannot be matched or safely replaced, use the official [FigureIDE
releases page](https://github.com/newkor3185-blip/FigureIDE/releases).

## Known limitations

- Automatic replacement does not support Windows installer installs, Linux
  package managers/system directories, or macOS bundles.
- Archive verification and package structure checks reduce corruption and path
  traversal risk but do not prove who published a release.
- Updates are checked only when the desktop application is running and can reach
  GitHub. Network errors and API rate limits do not prevent startup.
