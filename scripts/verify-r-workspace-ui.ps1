param(
    [ValidateSet('Auto', 'Desktop', 'Headless')][string]$Mode = 'Auto',
    [string]$Artifacts = 'target/r-workspace-ui',
    [switch]$SkipBuild,
    [string]$Executable = '',
    [string]$WorkingDirectory = ''
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $repo
$artifactRoot = if ([IO.Path]::IsPathRooted($Artifacts)) { [IO.Path]::GetFullPath($Artifacts) } else { [IO.Path]::GetFullPath((Join-Path $repo $Artifacts)) }
[void][IO.Directory]::CreateDirectory($artifactRoot)
$utf8 = [Text.UTF8Encoding]::new($false, $true)
function Write-Json($Path, $Value) { [IO.File]::WriteAllText($Path, ($Value | ConvertTo-Json -Depth 30), $utf8) }

$environment = [ordered]@{ os = [Environment]::OSVersion.ToString(); userInteractive = [Environment]::UserInteractive;
    sessionId = (Get-Process -Id $PID).SessionId; mode = $Mode; inputDesktop = $null; windowStation = $null; nativeAvailable = $false;
    inheritedLocale = @{ LC_ALL = $env:LC_ALL; LC_CTYPE = $env:LC_CTYPE; LANG = $env:LANG; LANGUAGE = $env:LANGUAGE } }
if ($env:OS -eq 'Windows_NT' -and $Mode -ne 'Headless') {
    if (-not ('RWorkspaceNative' -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class RWorkspaceNative {
 [DllImport("user32.dll",SetLastError=true)] public static extern IntPtr OpenInputDesktop(uint flags,bool inherit,uint access);
 [DllImport("user32.dll")] public static extern bool CloseDesktop(IntPtr desktop);
 [DllImport("user32.dll")] public static extern IntPtr GetProcessWindowStation();
 [DllImport("user32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool GetUserObjectInformation(IntPtr handle,int index,StringBuilder value,int length,out int needed);
 [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr hwnd,ref Point point);
 [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr hwnd,out Rect rect);
 [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr hwnd,IntPtr insertAfter,int x,int y,int width,int height,uint flags);
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
 public struct Point { public int X; public int Y; }
 public struct Rect { public int Left; public int Top; public int Right; public int Bottom; }
 public static bool PhysicalClientToScreen(IntPtr hwnd,ref Point point) {
   var previous=SetThreadDpiAwarenessContext(new IntPtr(-4));
   try { return ClientToScreen(hwnd,ref point); }
   finally { SetThreadDpiAwarenessContext(previous); }
 }
 public static string Name(IntPtr handle) { var name=new StringBuilder(256); int needed; return GetUserObjectInformation(handle,2,name,512,out needed) ? name.ToString() : "error:"+Marshal.GetLastWin32Error(); }
}
'@
    }
    $desktop = [RWorkspaceNative]::OpenInputDesktop(0, $false, 1)
    $environment.windowStation = [RWorkspaceNative]::Name([RWorkspaceNative]::GetProcessWindowStation())
    if ($desktop -ne [IntPtr]::Zero) {
        $environment.inputDesktop = [RWorkspaceNative]::Name($desktop)
        [void][RWorkspaceNative]::CloseDesktop($desktop)
    } else { $environment.inputDesktop = 'unavailable:' + [Runtime.InteropServices.Marshal]::GetLastWin32Error() }
    $environment.nativeAvailable = $environment.userInteractive -and $environment.windowStation -eq 'WinSta0' -and $environment.inputDesktop -eq 'Default'
}
Write-Json (Join-Path $artifactRoot 'environment.json') $environment
Write-Host ($environment | ConvertTo-Json -Compress)

if ($Mode -ne 'Desktop') {
    $oldArtifacts = $env:R_WORKSPACE_UI_ARTIFACTS
    $oldCapture = $env:R_WORKSPACE_UI_CAPTURE
    try {
        $env:R_WORKSPACE_UI_ARTIFACTS = Join-Path $artifactRoot 'headless'
        $env:R_WORKSPACE_UI_CAPTURE = $null
        & cargo test --offline -p vectorcraft-ui-egui --lib r_workspace_ -- --test-threads=1
        if ($LASTEXITCODE -ne 0) { throw "Headless UI regression tests failed ($LASTEXITCODE)" }
        # Separate optional capture pass: a missing GPU must not erase geometry verification.
        # Independent wgpu devices must not race during adapter startup on Windows.
        $env:R_WORKSPACE_UI_CAPTURE = '1'
        & cargo test --offline -p vectorcraft-ui-egui --lib r_workspace_ -- --test-threads=1
        $captureExit = $LASTEXITCODE
        Write-Json (Join-Path $artifactRoot 'offscreen-capture-result.json') @{ exitCode = $captureExit; geometryPassed = $true }
        if ($captureExit -ne 0) { Write-Warning 'Offscreen capture failed; geometry results remain available. See offscreen-capture-result.json.' }
    } finally { $env:R_WORKSPACE_UI_ARTIFACTS = $oldArtifacts; $env:R_WORKSPACE_UI_CAPTURE = $oldCapture }
}
if ($Mode -eq 'Headless') { return }
if (-not $environment.nativeAvailable) {
    if ($Mode -eq 'Desktop') { throw 'No accessible interactive Windows input desktop; use -Mode Headless.' }
    Write-Host 'Native desktop unavailable; headless geometry and offscreen captures completed.'
    return
}
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
if (-not $SkipBuild) {
    & cargo build --offline -p vectorcraft --bin vectorcraft
    if ($LASTEXITCODE -ne 0) { throw "Desktop build failed ($LASTEXITCODE)" }
}
$nativeRoot = Join-Path $artifactRoot 'desktop'
[void][IO.Directory]::CreateDirectory($nativeRoot)
$listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
$listener.Start(); $port = $listener.LocalEndpoint.Port; $listener.Stop()
$start = [Diagnostics.ProcessStartInfo]::new()
$start.FileName = if ($Executable) { [IO.Path]::GetFullPath($Executable) } else { Join-Path $repo 'target/debug/vectorcraft.exe' }
$start.Arguments = '--control ' + $port
$start.WorkingDirectory = if ($WorkingDirectory) { [IO.Path]::GetFullPath($WorkingDirectory) } else { $repo }
if (-not (Test-Path -LiteralPath $start.FileName -PathType Leaf)) { throw "Application executable is missing: $($start.FileName)" }
$start.UseShellExecute = $false
$start.CreateNoWindow = $true # Hide the debug console subsystem; the native GUI still opens.
$start.RedirectStandardOutput = $true
$start.RedirectStandardError = $true
$start.EnvironmentVariables['VECTORCRAFT_NO_PREFS'] = '1'
$start.EnvironmentVariables['VECTORCRAFT_CONFIG_DIR'] = Join-Path $nativeRoot 'profile'
$removedLocale = @()
foreach ($key in @($start.EnvironmentVariables.Keys)) {
    if (($key.StartsWith('LC_') -or $key -eq 'LANG' -or $key -eq 'LANGUAGE') -and $start.EnvironmentVariables[$key] -match '^C[.]UTF-?8$') {
        # The agent's Unix locale is unsupported by Windows R. Use the Windows user default in
        # this isolated child; preserve the caller's environment and all other locale choices.
        $start.EnvironmentVariables.Remove($key)
        $removedLocale += $key
    }
}
Write-Json (Join-Path $nativeRoot 'launch-environment.json') @{ removedUnsupportedLocaleOverrides = $removedLocale; policy = 'Windows user default for agent C.UTF-8 overrides' }
$appProcess = [Diagnostics.Process]::Start($start)
$stdout = $appProcess.StandardOutput.ReadToEndAsync()
$stderr = $appProcess.StandardError.ReadToEndAsync()
$script:requestId = 0
$script:transcript = [Collections.Generic.List[string]]::new()
$script:captures = [Collections.Generic.List[object]]::new()
function Invoke-Control([string]$Method, $Params = @{}) {
    $script:requestId++
    $client = [Net.Sockets.TcpClient]::new()
    try {
        $client.Connect('127.0.0.1', $port)
        $client.ReceiveTimeout = 15000
        $stream = $client.GetStream()
        $reader = [IO.StreamReader]::new($stream, $utf8)
        $writer = [IO.StreamWriter]::new($stream, $utf8)
        $writer.AutoFlush = $true
        $request = @{ id = $script:requestId; method = $Method; params = $Params } | ConvertTo-Json -Depth 20 -Compress
        $writer.WriteLine($request)
        $line = $reader.ReadLine()
        $script:transcript.Add($request); $script:transcript.Add($line)
        if (-not $line) { throw "No response for $Method" }
        $response = $line | ConvertFrom-Json
        if (-not $response.ok) { throw "$Method failed: $($response.error)" }
        return $response.result
    } finally { $client.Dispose() }
}
function Wait-State([scriptblock]$Predicate, [string]$Description, [int]$TimeoutSeconds = 40) {
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    do {
        if ($appProcess.HasExited) { throw "App exited during $Description (exit $($appProcess.ExitCode))" }
        $state = Invoke-Control 'ui.inspect'
        if (& $Predicate $state) { return $state }
        Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "Timed out waiting for $Description; R status: $($state.rWorkspace.status)"
}
function Click-Rect($Rect) {
    $state = Invoke-Control 'ui.inspect'
    if ($null -eq $Rect -or $Rect.Count -ne 4 -or $Rect[2] -le 0 -or $Rect[3] -le 0) { throw 'Missing or invalid widget rectangle' }
    $x = $Rect[0] + $Rect[2] / 2; $y = $Rect[1] + $Rect[3] / 2
    if ($x -lt 0 -or $y -lt 0 -or $x -ge $state.window[0] -or $y -ge $state.window[1]) { throw 'Widget is outside the current client viewport' }
    $null = Invoke-Control 'ui.click' @{ x = $x; y = $y }
}
function Click-Named([string]$Name, [int]$Index = 0) {
    $appProcess.Refresh()
    $root = [Windows.Automation.AutomationElement]::FromHandle($appProcess.MainWindowHandle)
    $condition = [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty, $Name)
    $deadline = [DateTime]::UtcNow.AddSeconds(5)
    do {
        $elements = @($root.FindAll([Windows.Automation.TreeScope]::Descendants, $condition))
        $elements = @($elements | Where-Object { -not $_.Current.IsOffscreen -and $_.Current.IsEnabled })
        if ($elements.Count -gt $Index) { break }
        Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $deadline)
    if ($elements.Count -le $Index) { throw "UI Automation could not find reachable '$Name' #$Index" }
    $element = $elements[$Index].Current
    $origin = [RWorkspaceNative+Point]::new()
    $clientRect = [RWorkspaceNative+Rect]::new()
    if (-not [RWorkspaceNative]::PhysicalClientToScreen($appProcess.MainWindowHandle, [ref]$origin) -or -not [RWorkspaceNative]::GetClientRect($appProcess.MainWindowHandle, [ref]$clientRect)) { throw 'Cannot measure the client window' }
    $state = Invoke-Control 'ui.inspect'
    $scale = $state.rWorkspace.pixelsPerPoint
    if ([Math]::Abs($clientRect.Right / $scale - $state.window[0]) -gt 2) { throw 'UIA/client pixel scale mismatch' }
    $rect = $element.BoundingRectangle
    $script:transcript.Add((@{ type = 'uia-measurement'; label = $Name; index = $Index; physicalRect = @($rect.X, $rect.Y, $rect.Width, $rect.Height);
        clientOrigin = @($origin.X, $origin.Y); pixelsPerPoint = $scale } | ConvertTo-Json -Compress))
    Click-Rect @( (($rect.X - $origin.X) / $scale), (($rect.Y - $origin.Y) / $scale), ($rect.Width / $scale), ($rect.Height / $scale) )
}
function Drag-Rect($Rect, [double]$Dx, [double]$Dy) {
    if ($null -eq $Rect -or $Rect.Count -ne 4) { throw 'Missing divider rectangle' }
    $x=$Rect[0]+$Rect[2]/2; $y=$Rect[1]+[Math]::Min($Rect[3]/2,80)
    $script:transcript.Add((@{type='control-drag';start=@($x,$y);delta=@($Dx,$Dy);steps=8} | ConvertTo-Json -Compress))
    # The application's raw-input hook delivers each pointer step on a separate
    # settled native UI frame. Keep all gestures on that supported input path.
    $null=Invoke-Control 'ui.drag' @{x=$x;y=$y;toX=($x+$Dx);toY=($y+$Dy);steps=8}
}
function Capture-Tab([string]$Tab, [int]$Width, [string]$Scenario = 'fixture') {
    $null = Invoke-Control 'ui.set' @{ rTab = $Tab; rWorkspaceWidth = $Width }
    $state = Wait-State { param($s) $s.rWorkspace.tab -eq $Tab -and -not $s.rWorkspace.running } "$Tab layout"
    $scale = [int][Math]::Round($state.rWorkspace.pixelsPerPoint * 100)
    $stem = "$Tab-${Width}px-scale$scale-$Scenario"
    $capture = Invoke-Control 'ui.screenshot' @{ path = (Join-Path $nativeRoot "$stem.png") }
    $state = Invoke-Control 'ui.inspect'
    Write-Json (Join-Path $nativeRoot "$stem.json") $state
    $script:captures.Add(@{ tab = $Tab; width = $Width; scale = $scale; image = $capture.path })
}
$succeeded = $false
try {
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    do {
        try { $state = Invoke-Control 'ui.inspect'; break } catch { if ($appProcess.HasExited -or [DateTime]::UtcNow -ge $deadline) { throw }; Start-Sleep -Milliseconds 100 }
    } while ($true)
    $appProcess.Refresh()
    [void][RWorkspaceNative]::SetWindowPos($appProcess.MainWindowHandle, [IntPtr]::Zero, 20, 20, 0, 0, 5)
    $null = Invoke-Control 'ui.resize' @{ width = 1600; height = 1000 }
    $null = Invoke-Control 'ui.set' @{ rWorkspace = $true; rWorkspaceWidth = 420; rTab = 'console'; rEditorFraction = 0.35; brightness = 'MediumDark' }
    $null = Invoke-Control 'engine.execute' @{ command = 'file.new'; params = @{ width = 900; height = 700 } }
    $null = Invoke-Control 'ui.screenshot' @{ path = (Join-Path $nativeRoot 'setup-native.png') }
    $state = Wait-State { param($s) $null -ne $s.rWorkspace.workspaceDividerRect } 'workspace divider'
    Drag-Rect $state.rWorkspace.workspaceDividerRect 80 0
    $state = Wait-State { param($s) [Math]::Abs($s.rWorkspace.width - 500) -lt 3 -and [Math]::Abs($s.rWorkspace.workspaceRect[2]-500) -lt 3 } 'real horizontal divider drag'
    Capture-Tab 'console' 500 'stage3-width-drag'
    $state = Invoke-Control 'ui.inspect'
    Click-Rect $state.rWorkspace.workspaceDividerRect
    $null = Invoke-Control 'ui.key' @{ key = 'Right' }
    $state = Wait-State { param($s) [Math]::Abs($s.rWorkspace.width - 516) -lt 3 -and [Math]::Abs($s.rWorkspace.workspaceRect[2]-516) -lt 3 } 'keyboard divider adjustment'
    $rect = $state.rWorkspace.workspaceDividerRect
    Click-Named 'Workspace'
    Click-Named 'Reset pane sizes'
    $state = Wait-State { param($s) [Math]::Abs($s.rWorkspace.width - 420) -lt 1 -and [Math]::Abs($s.rWorkspace.workspaceRect[2]-420) -lt 1 } 'workspace divider reset'
    $null = Invoke-Control 'ui.set' @{ rEditorFraction = 0.35 }
    $null = Invoke-Control 'ui.screenshot' @{ path = (Join-Path $nativeRoot 'stage3-reset-ready.png') }
    $state = Invoke-Control 'ui.inspect'
    Drag-Rect $state.rWorkspace.editorDividerRect 0 -50
    $state = Wait-State { param($s) $s.rWorkspace.editorFraction -lt 0.32 } 'real Source Console divider drag' 5
    Capture-Tab 'console' 420 'stage3-height-drag'
    $state = Invoke-Control 'ui.inspect'
    $rect = $state.rWorkspace.editorDividerRect
    Click-Named 'Workspace'
    Click-Named 'Reset pane sizes'
    $null = Wait-State { param($s) [Math]::Abs($s.rWorkspace.editorFraction - 0.62) -lt 0.01 } 'Source divider reset'
    Click-Named 'Maximize editor'
    $null = Wait-State { param($s) $s.rWorkspace.editorMaximized -and $null -eq $s.rWorkspace.editorDividerRect } 'maximized Source'
    Capture-Tab 'console' 420 'stage3-maximized-source'
    Click-Named 'Restore editor'
    $null = Invoke-Control 'ui.set' @{ rEditorFraction = 0.35 }
    foreach ($windowWidth in @(760,1000,1600)) {
        $null = Invoke-Control 'ui.resize' @{ width = $windowWidth; height = 1000 }
        $state = Wait-State { param($s) [Math]::Abs($s.window[0]-$windowWidth) -lt 2 } 'main window resize'
        Capture-Tab 'console' 420 "stage3-window$windowWidth"
        $state = Invoke-Control 'ui.inspect'
        if ($state.rWorkspace.vectorRect[0]+$state.rWorkspace.vectorRect[2] -gt $state.window[0]+2 -or $state.canvasRect[2] -le 0) { throw 'Narrow main window lost or overflowed its vector canvas' }
    }
    $null = Invoke-Control 'engine.execute' @{ command = 'window.toolbarColumns'; params = @{ double = $true } }
    $null = Wait-State { param($s) $s.rWorkspace.toolsRect[2] -ge 87 } 'double-column tools'
    Capture-Tab 'console' 420 'stage3-tools-double'
    $null = Invoke-Control 'engine.execute' @{ command = 'window.panel.float'; params = @{ panel = 'tools'; x = 650; y = 200 } }
    $null = Wait-State { param($s) $null -eq $s.rWorkspace.toolsRect } 'floating tools release dock width'
    Capture-Tab 'console' 420 'stage3-tools-floating'
    $null = Invoke-Control 'engine.execute' @{ command = 'window.panel.dock'; params = @{ panel = 'tools' } }
    $null = Invoke-Control 'engine.execute' @{ command = 'window.toolbarColumns'; params = @{ double = $false } }
    $state = Wait-State { param($s) $null -ne $s.rWorkspace.editorRect } 'script editor'
    Click-Rect $state.rWorkspace.editorRect
    $null = Wait-State { param($s) $s.rWorkspace.editorFocused } 'editor focus'
    $null = Invoke-Control 'ui.key' @{ key = 'A'; cmd = $true; ctrl = $true }
    $fixture = [IO.File]::ReadAllText((Join-Path $PSScriptRoot 'fixtures/r-workspace-ui.R'), $utf8).Replace("`r`n", "`n").Replace("`r", "`n")
    $null = Invoke-Control 'ui.text' @{ text = $fixture }
    $fixtureBytes = $utf8.GetByteCount($fixture)
    $null = Wait-State { param($s) $s.rWorkspace.scriptBytes -eq $fixtureBytes } 'fixture input applied to editor'
    $null = Invoke-Control 'ui.screenshot' @{ path = (Join-Path $nativeRoot 'fixture-in-editor-native.png') }
    $null = Invoke-Control 'ui.key' @{ key = 'Enter'; cmd = $true; ctrl = $true; shift = $true }
    $state = Wait-State { param($s) -not $s.rWorkspace.running -and $s.rWorkspace.plots.Count -eq 2 -and $s.rWorkspace.consoleOutput.Contains('R_WORKSPACE_UI_FIXTURE_READY') } 'real R fixture execution'
    if ($state.rWorkspace.consoleError) { throw "R fixture error: $($state.rWorkspace.status)" }
    $koreanName = -join @([char]0xD55C, [char]0xAE00, '_', [char]0xBCC0, [char]0xC218)
    $koreanObject = @($state.rWorkspace.objects | Where-Object { $_.variable -eq $koreanName })
    if ($koreanObject.Count -ne 1 -or -not $koreanObject[0].preview.Contains((-join @([char]0xD55C, [char]0xAE00)))) { throw 'Korean variable/value was not preserved in R metadata' }
    $null = Invoke-Control 'ui.set' @{ rTab = 'console' }
    $state = Wait-State { param($s) $null -ne $s.rWorkspace.consoleInputRect } 'Console input'
    Click-Rect $state.rWorkspace.consoleInputRect
    $null = Wait-State { param($s) $s.rWorkspace.consoleFocused } 'Console focus'
    $null = Invoke-Control 'ui.text' @{ text = 'numeric_value + 1' }
    $null = Wait-State { param($s) $s.rWorkspace.consoleInput -eq 'numeric_value + 1' } 'Console input applied'
    $null = Invoke-Control 'ui.key' @{ key = 'Enter' }
    $null = Wait-State { param($s) -not $s.rWorkspace.running -and $s.rWorkspace.consoleOutput.Contains('[1] 43') } 'shared Console execution'
    # Stage 2: actual multi-tab Source focus, unsaved state and shared Console history.
    Click-Named 'New R script'
    $state = Wait-State { param($s) $s.rWorkspace.scripts.Count -eq 2 -and $s.rWorkspace.editorFocused } 'second script focus'
    $secondScriptId = $state.rWorkspace.activeScriptId
    $sourceDraft = '# ' + $koreanName + "`n" + 'stage2_draft <- 7' + "`n"
    $null = Invoke-Control 'ui.text' @{ text = $sourceDraft }
    $null = Wait-State { param($s) $s.rWorkspace.scriptBytes -eq $utf8.GetByteCount($sourceDraft) -and @($s.rWorkspace.scripts | Where-Object { $_.id -eq $secondScriptId -and $_.dirty }).Count -eq 1 } 'unsaved script draft'
    foreach ($width in @(320, 420, 560, 800)) { Capture-Tab 'console' $width 'source-second-tab' }
    $state = Invoke-Control 'ui.inspect'
    if ($state.rWorkspace.consoleInputRect[2] -lt 100) { throw 'Long output scrolled the live Console input out of view' }
    $null = Invoke-Control 'ui.key' @{ key = 'Tab'; cmd = $true; ctrl = $true; shift = $true }
    $state = Wait-State { param($s) $s.rWorkspace.activeScriptId -ne $secondScriptId -and $s.rWorkspace.editorFocused } 'previous script shortcut'
    foreach ($width in @(320, 420, 560, 800)) { Capture-Tab 'console' $width 'source-first-tab' }
    Click-Named 'Console'
    $null = Wait-State { param($s) $s.rWorkspace.consoleFocused } 'Console tab focuses input'
    $null = Invoke-Control 'ui.text' @{ text = 'stage2 draft' }
    $null = Wait-State { param($s) $s.rWorkspace.consoleInput -eq 'stage2 draft' } 'Console draft'
    $null = Invoke-Control 'ui.key' @{ key = 'Up' }
    $null = Wait-State { param($s) $s.rWorkspace.consoleInput -eq 'numeric_value + 1' } 'Console history up'
    $null = Invoke-Control 'ui.key' @{ key = 'Down' }
    $null = Wait-State { param($s) $s.rWorkspace.consoleInput -eq 'stage2 draft' } 'Console history restores draft'
    $null = Invoke-Control 'ui.key' @{ key = 'Escape' }
    $null = Wait-State { param($s) $s.rWorkspace.consoleInput -eq '' } 'Console draft cancel'
    $longOutputCode = 'cat(paste(sprintf("STAGE2_OUTPUT_%03d", 1:160), collapse="\n"), "\n")'
    $null = Invoke-Control 'ui.text' @{ text = $longOutputCode }
    $null = Wait-State { param($s) $s.rWorkspace.consoleInput -eq $longOutputCode } 'long output code input'
    $null = Invoke-Control 'ui.key' @{ key = 'Enter' }
    $state = Wait-State { param($s) -not $s.rWorkspace.running -and $s.rWorkspace.consoleOutput.Contains('STAGE2_OUTPUT_160') } 'real Console long output'
    Capture-Tab 'console' 320 'long-output'
    $state = Invoke-Control 'ui.inspect'
    $null = Invoke-Control 'ui.wheel' @{ x = 80; y = ($state.rWorkspace.consoleInputRect[1] - 100); dy = 100 }
    $null = Wait-State { param($s) -not $s.rWorkspace.consoleScrollAtBottom -and $s.rWorkspace.consoleFocused } 'scrollback retains Console focus'
    Capture-Tab 'console' 320 'scrollback'
    $multiline = "stage2_multiline <- function(x) {`n  x + 3`n}`nstage2_multiline(4)"
    $null = Invoke-Control 'ui.text' @{ text = $multiline }
    $null = Wait-State { param($s) $s.rWorkspace.consoleInput -eq $multiline -and $s.rWorkspace.consoleInputRect[3] -gt 0 } 'multiline paste reveals live input'
    $null = Invoke-Control 'ui.key' @{ key = 'Enter' }
    $null = Wait-State { param($s) -not $s.rWorkspace.running -and $s.rWorkspace.consoleOutput.Contains('[1] 7') -and @($s.rWorkspace.objects | Where-Object { $_.variable -eq 'stage2_multiline' }).Count -eq 1 } 'multiline Console execution'
    $null = Invoke-Control 'ui.set' @{ rTab = 'plots'; rWorkspaceWidth = 420 }
    $null = Invoke-Control 'ui.screenshot' @{ path = (Join-Path $nativeRoot 'plots-before-selection.png') }
    Click-Named 'Select' 0
    $null = Wait-State { param($s) $s.rWorkspace.selectedPlot -eq 'plot_A' } 'Plot Registry selection'
    foreach ($width in @(320, 420, 560, 800)) {
        Capture-Tab 'environment' $width
        Capture-Tab 'plots' $width
        Capture-Tab 'console' $width
    }
    $null = Invoke-Control 'ui.set' @{ rTab = 'environment'; rWorkspaceWidth = 320 }
    $null = Invoke-Control 'ui.screenshot' @{ path = (Join-Path $nativeRoot 'environment-before-filter.png') }
    $appProcess.Refresh()
    $root = [Windows.Automation.AutomationElement]::FromHandle($appProcess.MainWindowHandle)
    $editCondition = [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty, [Windows.Automation.ControlType]::Edit)
    $emptyEdits = @($root.FindAll([Windows.Automation.TreeScope]::Descendants, $editCondition) | Where-Object {
        $pattern = $null
        -not $_.Current.IsOffscreen -and $_.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern, [ref]$pattern) -and $pattern.Current.Value -eq ''
    })
    if ($emptyEdits.Count -ne 1) { throw 'The Environment filter is not uniquely accessible through UI Automation' }
    $filter = $emptyEdits[0]
    $filter.SetFocus()
    $deadline = [DateTime]::UtcNow.AddSeconds(5)
    while (-not $filter.Current.HasKeyboardFocus -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 100 }
    if (-not $filter.Current.HasKeyboardFocus) { throw 'Environment filter could not receive focus' }
    $null = Invoke-Control 'ui.text' @{ text = $koreanName }
    $deadline = [DateTime]::UtcNow.AddSeconds(5)
    do {
        $filterValue = $filter.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).Current.Value
        if ($filterValue -eq $koreanName) { break }
        Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $deadline)
    if ($filterValue -ne $koreanName) { throw 'Korean filter text was not applied' }
    Capture-Tab 'environment' 320 'unicode-filter'
    $null = Invoke-Control 'ui.set' @{ rTab = 'plots'; rWorkspaceWidth = 560 }
    $null = Wait-State { param($s) $s.rWorkspace.tab -eq 'plots' } 'Plot Registry'
    Click-Named 'Add to Artboard' 0
    $null = Wait-State { param($s) -not $s.rWorkspace.running -and $s.rWorkspace.linkedPanels.Count -eq 1 } 'first SVG plot insertion'
    Click-Named 'Add to Artboard' 1
    $null = Wait-State { param($s) -not $s.rWorkspace.running -and $s.rWorkspace.linkedPanels.Count -eq 2 } 'second SVG plot insertion'
    foreach ($width in @(320, 420, 560, 800)) { Capture-Tab 'linked' $width }
    $linkedState = Invoke-Control 'ui.inspect'
    $panelId = $linkedState.rWorkspace.linkedPanels[0].id
    $null = Invoke-Control 'engine.execute' @{ command = 'select.set'; params = @{ ids = @($panelId) } }
    Capture-Tab 'console' 420 'stage4-inspector-linked-before-update'
    # The Console tab has no duplicate R property controls, so this reachable
    # action comes from the native Inspector and proves the selected link is shown.
    Click-Named 'Update from R'
    $null = Wait-State { param($s) -not $s.rWorkspace.running -and $s.rWorkspace.status.StartsWith('Updated linked panel') } 'Inspector linked SVG refresh'
    Capture-Tab 'console' 420 'stage4-inspector-linked-after-update'
    $null = Invoke-Control 'ui.resize' @{ width = 760; height = 1000 }
    $state = Wait-State { param($s) [Math]::Abs($s.window[0]-760) -lt 2 -and $null -ne $s.taskBar.rect } 'narrow selected plot layout'
    $bar = $state.taskBar.rect; $canvas = $state.canvasRect
    if ($bar[0] -lt $canvas[0]-1 -or $bar[0]+$bar[2] -gt $canvas[0]+$canvas[2]+1) { throw 'Selection actions overflowed the narrow vector canvas' }
    Capture-Tab 'console' 420 'stage5-narrow-selected-inspector'
    Click-Named ([string][char]0x2192)
    Capture-Tab 'console' 420 'stage5-narrow-inspector-scrolled'
    Click-Named ([string][char]0x2190)
    $null = Invoke-Control 'ui.resize' @{ width = 1600; height = 1000 }
    $null = Wait-State { param($s) [Math]::Abs($s.window[0]-1600) -lt 2 } 'restore wide selected plot layout'
    $null = Invoke-Control 'ui.set' @{ rTab = 'linked' }
    $null = Wait-State { param($s) $s.rWorkspace.tab -eq 'linked' } 'Linked Plot Panel after Inspector refresh'
    Click-Named 'Update' 0
    $null = Wait-State { param($s) -not $s.rWorkspace.running -and $s.rWorkspace.status.StartsWith('Updated linked panel') } 'linked SVG refresh'
    Click-Named 'Go to Source' 0
    $state = Wait-State { param($s) $s.rWorkspace.editorFocused -and $s.rWorkspace.activeScriptId -ne $secondScriptId -and $s.rWorkspace.status.StartsWith('Opened') } 'linked plot source navigation'
    if ($state.rWorkspace.selectedPlot -ne 'plot_A' -or $state.rWorkspace.linkedPanels.Count -ne 2) { throw 'Source navigation changed plot selection or linkage' }
    $null = Invoke-Control 'ui.screenshot' @{ path = (Join-Path $nativeRoot 'linked-source-jump-native.png') }
    $shape = Invoke-Control 'engine.execute' @{ command = 'shape.rectangle'; params = @{ x = 20; y = 20; width = 50; height = 30 } }
    $null = Invoke-Control 'engine.execute' @{ command = 'object.move'; params = @{ ids = @($shape.id); dx = 15; dy = 10 } }
    Click-Named 'New R script'
    $null = Wait-State { param($s) $s.rWorkspace.editorFocused -and $s.rWorkspace.scripts.Count -eq 3 } 'navigation test script focus'
    $navigationCode = "x <- 10`n`n# note`ny <- x * 2"
    $null = Invoke-Control 'ui.text' @{ text = $navigationCode }
    $null = Wait-State { param($s) $s.rWorkspace.scriptBytes -eq $utf8.GetByteCount($navigationCode) } 'navigation test input'
    $null = Invoke-Control 'ui.key' @{ key = 'Home'; cmd = $true; ctrl = $true }
    $null = Wait-State { param($s) $s.rWorkspace.editorCursorChar -eq 0 } 'navigation start cursor'
    $null = Invoke-Control 'ui.key' @{ key = 'Enter'; cmd = $true; ctrl = $true }
    $nextExpressionChar = "x <- 10`n`n# note`n".Length
    $state = Wait-State { param($s) -not $s.rWorkspace.running -and $s.rWorkspace.editorFocused -and $s.rWorkspace.editorCursorChar -eq $nextExpressionChar -and @($s.rWorkspace.objects | Where-Object { $_.variable -eq 'x' }).Count -eq 1 } 'Ctrl+Enter skips comments and blank lines'
    if ($state.rWorkspace.scriptBytes -ne $utf8.GetByteCount($navigationCode)) { throw 'First Ctrl+Enter changed source text' }
    $null = Invoke-Control 'ui.screenshot' @{ path = (Join-Path $nativeRoot 'source-next-expression-native.png') }
    $null = Invoke-Control 'ui.key' @{ key = 'Enter'; cmd = $true; ctrl = $true }
    $expectedBytes = $utf8.GetByteCount($navigationCode) + 1
    $expectedCursor = $navigationCode.Length + 1
    $state = Wait-State { param($s) -not $s.rWorkspace.running -and $s.rWorkspace.editorFocused -and $s.rWorkspace.editorCursorChar -eq $expectedCursor -and $s.rWorkspace.scriptBytes -eq $expectedBytes -and @($s.rWorkspace.objects | Where-Object { $_.variable -eq 'y' }).Count -eq 1 } 'Ctrl+Enter creates one EOF blank line'
    $null = Invoke-Control 'ui.screenshot' @{ path = (Join-Path $nativeRoot 'source-eof-caret-native.png') }
    # Real scientific workflow: inspect categorized metadata and render two plot kinds.
    Click-Named 'New R script'
    $workflow = [IO.File]::ReadAllText((Join-Path $PSScriptRoot 'fixtures/r-workflow-compatibility.R'), $utf8).Replace("`r`n", "`n").Replace("`r", "`n")
    $null = Invoke-Control 'ui.text' @{ text = $workflow }
    $null = Wait-State { param($s) $s.rWorkspace.scriptBytes -eq $utf8.GetByteCount($workflow) } 'scientific workflow source input'
    $null = Invoke-Control 'ui.key' @{ key = 'Enter'; cmd = $true; ctrl = $true; shift = $true }
    $state = Wait-State { param($s) -not $s.rWorkspace.running -and $s.rWorkspace.consoleOutput.Contains('R_WORKFLOW_FIXTURE_READY') -and $s.rWorkspace.plots.Count -eq 4 } 'scientific workflow evaluation'
    foreach ($item in @(@('workflow_data', 'Data'), @('workflow_matrix', 'Data'), @('workflow_function', 'Functions'), @('workflow_dot', 'Values'))) {
        $match = @($state.rWorkspace.objects | Where-Object { $_.variable -eq $item[0] -and $_.category -eq $item[1] })
        if ($match.Count -ne 1 -or [string]::IsNullOrWhiteSpace($match[0].description)) { throw "Incorrect Environment metadata for $($item[0])" }
    }
    $null = Invoke-Control 'ui.set' @{ rTab = 'environment'; rWorkspaceWidth = 560 }
    $null = Wait-State { param($s) $s.rWorkspace.tab -eq 'environment' } 'scientific Environment tab'
    $appProcess.Refresh()
    $root = [Windows.Automation.AutomationElement]::FromHandle($appProcess.MainWindowHandle)
    $matchingFilters = @($root.FindAll([Windows.Automation.TreeScope]::Descendants, $editCondition) | Where-Object {
        $pattern = $null
        -not $_.Current.IsOffscreen -and $_.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern, [ref]$pattern) -and $pattern.Current.Value -eq $koreanName
    })
    if ($matchingFilters.Count -ne 1) { throw 'The active Environment filter was not uniquely accessible' }
    $filter = $matchingFilters[0]
    $filter.SetFocus()
    $null = Invoke-Control 'ui.key' @{ key = 'A'; cmd = $true; ctrl = $true }
    $null = Invoke-Control 'ui.key' @{ key = 'Backspace' }
    Capture-Tab 'environment' 560 'workflow-categories'
    $null = Invoke-Control 'ui.set' @{ rTab = 'plots'; rWorkspaceWidth = 560 }
    $state = Wait-State { param($s) $s.rWorkspace.tab -eq 'plots' -and $s.rWorkspace.plots.Count -eq 4 } 'scientific Plot Registry'
    $dotIndex = [array]::IndexOf([object[]]$state.rWorkspace.plots, 'workflow_dot')
    $heatmapIndex = [array]::IndexOf([object[]]$state.rWorkspace.plots, 'workflow_heatmap')
    if ($dotIndex -lt 0 -or $heatmapIndex -lt 0) { throw 'Scientific plots missing from registry' }
    Capture-Tab 'plots' 560 'workflow-registry'
    Click-Named 'Add to Artboard' $dotIndex
    $null = Wait-State { param($s) -not $s.rWorkspace.running -and $s.rWorkspace.linkedPanels.Count -eq 3 } 'gradient dot plot insertion'
    $null = Invoke-Control 'ui.screenshot' @{ path = (Join-Path $nativeRoot 'workflow-gradient-artboard.png') }
    Click-Named 'Add to Artboard' $heatmapIndex
    $null = Wait-State { param($s) -not $s.rWorkspace.running -and $s.rWorkspace.linkedPanels.Count -eq 4 } 'pheatmap insertion'
    $null = Invoke-Control 'ui.screenshot' @{ path = (Join-Path $nativeRoot 'workflow-heatmap-artboard.png') }
    Write-Json (Join-Path $nativeRoot 'document.json') (Invoke-Control 'document.inspect')
    $state = Invoke-Control 'ui.inspect'
    Write-Json (Join-Path $nativeRoot 'final-state.json') $state
    $succeeded = $true
} finally {
    if (-not $appProcess.HasExited) {
        try {
            $null = Invoke-Control 'app.quit'
            for ($attempt = 0; $attempt -lt 5 -and -not $appProcess.WaitForExit(500); $attempt++) {
                $state = Invoke-Control 'ui.inspect'
                if ($null -ne $state.ui.dialog) {
                    $null = Invoke-Control 'ui.dialog.set' @{ field = 'discard'; value = $true }
                    $null = Invoke-Control 'ui.dialog.confirm'
                }
            }
        } catch { Write-Warning "Clean shutdown: $_" }
        if (-not $appProcess.WaitForExit(5000)) { $appProcess.Kill(); $appProcess.WaitForExit(); $succeeded = $false; Write-Warning 'The test-owned process required forced termination.' }
    }
    [IO.File]::WriteAllText((Join-Path $nativeRoot 'stdout.log'), $stdout.Result, $utf8)
    [IO.File]::WriteAllText((Join-Path $nativeRoot 'stderr.log'), $stderr.Result, $utf8)
    [IO.File]::WriteAllLines((Join-Path $nativeRoot 'control.jsonl'), $script:transcript, $utf8)
    Write-Json (Join-Path $nativeRoot 'result.json') @{ success = $succeeded; pid = $appProcess.Id; exitCode = $appProcess.ExitCode; captures = $script:captures.ToArray(); nativeFrameCapture = ($script:captures.Count -gt 0); visuallyInspected = $false }
}
if (-not $succeeded) { throw 'Desktop verification failed; inspect the recorded artifacts.' }
Write-Host "Desktop verification passed. Screenshots require image review: $nativeRoot"
