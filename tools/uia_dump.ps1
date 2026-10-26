<#
.SYNOPSIS
    Dump the UI Automation tree of the foreground window: for developing MetaMate's pointing and for replay fixtures.

.DESCRIPTION
    Called by `mm uia`. Prints an indented tree (control type, name, rect); with -JsonPath also writes the
    nodes as JSON, with -ShotPath a PNG of the window. Rects are physical screen pixels when the process could
    switch to Per-Monitor DPI Aware v2 (reported as dpi_awareness). Control values (text boxes, passwords)
    are never read: only names, types and geometry.
#>
param(
    [ValidateRange(0, 60)][int]$Delay = 3,
    [ValidateRange(0, 64)][int]$Depth = 12,
    [ValidateRange(1, 10000)][int]$MaxNodes = 600,
    [string]$JsonPath = '',
    [string]$ShotPath = '',
    [switch]$SelfTest
)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

# Unicode escapes keep this BOM-less UTF-8 script safe in Windows PowerShell 5.1.
# Long words also mark a "show password" toggle; the short ones (PIN, OTP...) only edit fields:
# buttons such as "Pin tab" or "Pin to Quick access" must not refuse a screenshot.
$PasswordLong = ('(?i)password|passwd|passcode|passphrase|\bparol|' +
    'one[ -]time[ -]code|verification\s+code|security\s+code|' +
    '\u043f\u0430\u0440\u043e\u043b\u044c|\u043f\u0438\u043d-\u043a\u043e\u0434|kennwort|contrase\u00f1a|mot\s+de\s+passe|senha|' +
    '[\u015fs]ifre|has\u0142o|heslo|wachtwoord|l\u00f6senord|salasana|jelsz\u00f3|parola|' +
    '\u03ba\u03c9\u03b4\u03b9\u03ba\u03cc\u03c2|\u5bc6\u7801|\u5bc6\u78bc|\u30d1\u30b9\u30ef\u30fc\u30c9|\ube44\ubc00\ubc88\ud638|' +
    '\u0643\u0644\u0645\u0629\s+\u0627\u0644\u0645\u0631\u0648\u0631|\u05e1\u05d9\u05e1\u05de\u05d4|' +
    'm\u1eadt\s+kh\u1ea9u|kata\s+sandi|\u0e23\u0e2b\u0e31\u0e2a\u0e1c\u0e48\u0e32\u0e19')
# Short words count only as tokens: a lowercase letter next to them makes another word
# (Pinned, Spinner), while ids such as txt_pin, pinBox or PIN+CJK still count.
$PasswordShort = '(?<!(?-i:[a-z]))(?:pwd|pin|otp|cvv|cvc)(?!(?-i:[a-z]))|(?<![\u0430-\u044f\u0451])\u043f\u0438\u043d(?![\u0430-\u044f\u0451])'
# Culture-invariant case folding: under tr-TR, (?i) alone would fold I to a dotless i.
$RegexOptions = [System.Text.RegularExpressions.RegexOptions]'CultureInvariant'
$PasswordWords = New-Object System.Text.RegularExpressions.Regex(($PasswordLong + '|' + $PasswordShort), $RegexOptions)
$PasswordToggle = New-Object System.Text.RegularExpressions.Regex($PasswordLong, $RegexOptions)

# A revealed password is plain text with IsPassword false: also go by the field's label and
# by any control on screen that talks about the password (an eye button can be a Button,
# Custom, Image or Hyperlink; WinUI's is AutomationId RevealButton), and the PasswordBox class.
function Test-PasswordLike([string]$Type, [string]$Name, [string]$AutomationId, [string]$HelpText, [string]$ClassName) {
    if ($ClassName -eq 'PasswordBox') { return $true }
    if ($Type -eq 'Edit') {
        return $PasswordWords.IsMatch($Name) -or $PasswordWords.IsMatch($AutomationId) -or $PasswordWords.IsMatch($HelpText)
    }
    # Static labels ("Password:") and the window itself are not a sign that one is visible.
    if ($Type -in @('Text', 'Document', 'Window', 'Pane')) { return $false }
    return $AutomationId -eq 'RevealButton' -or $PasswordToggle.IsMatch($Name) -or $PasswordToggle.IsMatch($AutomationId)
}

if ($SelfTest) {
    $mustMatch = @('Password', 'Parol', '\u041f\u0430\u0440\u043e\u043b\u044c', 'PIN', 'Show password', 'Hide password',
        'passwd', 'passcode', 'passphrase', 'pwd', 'one-time code', 'OTP', 'verification code',
        '\u043f\u0438\u043d-\u043a\u043e\u0434', 'kennwort', 'contrase\u00f1a', 'mot de passe', 'senha',
        '\u015fifre', 'sifre', 'has\u0142o', 'heslo', 'wachtwoord', 'l\u00f6senord', 'salasana', 'jelsz\u00f3', 'parola',
        '\u03ba\u03c9\u03b4\u03b9\u03ba\u03cc\u03c2', '\u5bc6\u7801', '\u5bc6\u78bc', '\u30d1\u30b9\u30ef\u30fc\u30c9',
        '\ube44\ubc00\ubc88\ud638', '\u0643\u0644\u0645\u0629 \u0627\u0644\u0645\u0631\u0648\u0631',
        '\u05e1\u05d9\u05e1\u05de\u05d4', 'm\u1eadt kh\u1ea9u', 'kata sandi', '\u0e23\u0e2b\u0e31\u0e2a\u0e1c\u0e48\u0e32\u0e19',
        'CVV', 'CVC', 'security code', 'Parolingiz')
    $mustMatch += @('PIN\u7801', 'txt_pin', 'pinBox', 'otp_input', '\u041f\u0418\u041d', '\u043f\u0438\u043d')
    $mustNotMatch = @('Pinned', 'Passport', 'Spinner', 'Bypass', 'Login', 'Search site', '\u043f\u0438\u043d\u0430\u0442\u044c', 'Opinion', 'Shopping')
    # Toggles: the long words only, so a "Pin tab" button does not refuse a screenshot.
    $toggleMatch = @('Show password', 'Hide password', "Parolni ko'rsatish",
        '\u041f\u043e\u043a\u0430\u0437\u0430\u0442\u044c \u043f\u0430\u0440\u043e\u043b\u044c')
    $toggleNoMatch = @('Pin tab', 'Pin to Quick access', 'PIN', 'OTP')
    $mismatches = 0
    foreach ($sample in $mustMatch) {
        $label = [regex]::Unescape($sample)
        if (-not $PasswordWords.IsMatch($label)) { Write-Output "Expected match: $label"; $mismatches++ }
    }
    foreach ($sample in $mustNotMatch) {
        $label = [regex]::Unescape($sample)
        if ($PasswordWords.IsMatch($label)) { Write-Output "Unexpected match: $label"; $mismatches++ }
    }
    foreach ($sample in $toggleMatch) {
        $label = [regex]::Unescape($sample)
        if (-not $PasswordToggle.IsMatch($label)) { Write-Output "Expected toggle match: $label"; $mismatches++ }
    }
    foreach ($sample in $toggleNoMatch) {
        $label = [regex]::Unescape($sample)
        if ($PasswordToggle.IsMatch($label)) { Write-Output "Unexpected toggle match: $label"; $mismatches++ }
    }
    # expected, control type, name, automation id, help text, class name
    $nodeCases = @(
        @($true, 'Edit', '', 'password', '', ''),
        @($true, 'Edit', 'Parol', '', '', ''),
        @($false, 'Edit', 'Search site', '', '', ''),
        @($true, 'Custom', '', 'togglePassword', '', ''),
        @($true, 'Image', 'Show password', '', '', ''),
        @($true, 'Hyperlink', 'Forgot password?', '', '', ''),
        @($true, 'Button', '', 'RevealButton', '', ''),
        @($true, 'Edit', '', '', '', 'PasswordBox'),
        @($false, 'Button', 'Pin tab', '', '', ''),
        @($false, 'Text', 'Password', '', '', ''),
        @($false, 'Window', 'Change password', '', '', ''))
    foreach ($case in $nodeCases) {
        $got = Test-PasswordLike $case[1] $case[2] $case[3] $case[4] $case[5]
        if ($got -ne $case[0]) { Write-Output "Node case wrong: $($case[1..5] -join ' | ') -> $got"; $mismatches++ }
    }
    if ($mismatches -gt 0) { throw "Password words self-test FAILED: $mismatches mismatches." }
    Write-Output 'Password words self-test OK.'
    return
}

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class MmNative {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    public delegate bool EnumWindowsProc(IntPtr hwnd, IntPtr param);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr param);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr hwnd, uint command);
    [DllImport("user32.dll")] public static extern int GetWindowLongW(IntPtr hwnd, int index);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr hwnd, StringBuilder text, int max);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int index);
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr value);
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr value);
    [DllImport("user32.dll")] public static extern IntPtr GetThreadDpiAwarenessContext();
    [DllImport("user32.dll")] public static extern int GetAwarenessFromDpiAwarenessContext(IntPtr value);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr hwnd);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int max);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr hwnd, int attr, out RECT rect, int size);
    [DllImport("dwmapi.dll", EntryPoint = "DwmGetWindowAttribute")] public static extern int GetCloaked(IntPtr hwnd, int attr, out int cloaked, int size);
    public static IntPtr[] TopLevelWindows() {
        var windows = new List<IntPtr>();
        if (!EnumWindows(delegate(IntPtr hwnd, IntPtr param) { windows.Add(hwnd); return true; }, IntPtr.Zero)) {
            throw new InvalidOperationException("Cannot enumerate top-level windows.");
        }
        return windows.ToArray();
    }
}
'@

# -4 = DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2; fails harmlessly if the host already set an awareness.
[void][MmNative]::SetProcessDpiAwarenessContext([IntPtr](-4))
[void][MmNative]::SetThreadDpiAwarenessContext([IntPtr](-4))
$aw = [MmNative]::GetAwarenessFromDpiAwarenessContext([MmNative]::GetThreadDpiAwarenessContext())
$awareness = if ($aw -ge 0 -and $aw -le 2) { @('unaware', 'system', 'per-monitor')[$aw] } else { 'unknown' }

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Drawing

if ($Delay -gt 0) { Start-Sleep -Seconds $Delay }
$hwnd = [MmNative]::GetForegroundWindow()
if ($hwnd -eq [IntPtr]::Zero) { Write-Error 'No foreground window.' }

$title = New-Object System.Text.StringBuilder 512
[void][MmNative]::GetWindowText($hwnd, $title, 512)
$procId = [uint32]0
$threadId = [MmNative]::GetWindowThreadProcessId($hwnd, [ref]$procId)
$procName = try { (Get-Process -Id $procId).ProcessName } catch { '?' }
$frame = New-Object MmNative+RECT
# 9 = DWMWA_EXTENDED_FRAME_BOUNDS: the visible frame without the invisible resize borders.
if ([MmNative]::DwmGetWindowAttribute($hwnd, 9, [ref]$frame, 16) -ne 0) {
    throw 'Cannot obtain the foreground window bounds.'
}

function Get-Popups {
    $popups = New-Object 'System.Collections.Generic.List[object]'
    foreach ($popupHwnd in [MmNative]::TopLevelWindows()) {
        if ($popupHwnd -eq $hwnd -or -not [MmNative]::IsWindowVisible($popupHwnd)) { continue }
        $class = New-Object System.Text.StringBuilder 256
        [void][MmNative]::GetClassName($popupHwnd, $class, 256)
        $owned = $false
        $owner = $popupHwnd
        # 4 = GW_OWNER; bound the chain in case a window disappears or ownership changes.
        for ($i = 0; $i -lt 8; $i++) {
            $owner = [MmNative]::GetWindow($owner, 4)
            if ($owner -eq [IntPtr]::Zero) { break }
            if ($owner -eq $hwnd) { $owned = $true; break }
        }
        $popupProcId = [uint32]0
        $popupThreadId = [MmNative]::GetWindowThreadProcessId($popupHwnd, [ref]$popupProcId)
        $popupClass = $class.ToString() -in @('#32768', 'ComboLBox')
        # -16 = GWL_STYLE, -20 = GWL_EXSTYLE; Chromium menus are sometimes unowned.
        $chromePopup = $false
        if ($popupThreadId -eq $threadId -and $class.ToString().StartsWith('Chrome_WidgetWin_')) {
            $style = [MmNative]::GetWindowLongW($popupHwnd, -16)
            $exStyle = [MmNative]::GetWindowLongW($popupHwnd, -20)
            # WS_POPUP, WS_EX_TOOLWINDOW and no WS_EX_APPWINDOW exclude ordinary Chrome frames.
            $chromePopup = ($style -band 0x80000000) -ne 0 -and
                ($exStyle -band 0x80) -ne 0 -and ($exStyle -band 0x40000) -eq 0
        }
        if (-not $owned -and -not ($popupThreadId -eq $threadId -and ($popupClass -or $chromePopup))) { continue }
        $cloaked = 0
        # 14 = DWMWA_CLOAKED: invisible virtual-desktop windows must not enlarge the capture.
        if ([MmNative]::GetCloaked($popupHwnd, 14, [ref]$cloaked, 4) -ne 0) {
            if (-not [MmNative]::IsWindow($popupHwnd)) { continue }
            throw 'Cannot determine whether a popup is cloaked.'
        }
        if ($cloaked -ne 0) { continue }
        $popupFrame = New-Object MmNative+RECT
        # Some menu windows have no DWM frame; their window rect is the visible bounds.
        if ([MmNative]::DwmGetWindowAttribute($popupHwnd, 9, [ref]$popupFrame, 16) -ne 0 -and
            -not [MmNative]::GetWindowRect($popupHwnd, [ref]$popupFrame)) {
            if (-not [MmNative]::IsWindow($popupHwnd)) { continue }
            throw 'Cannot obtain a popup window bounds.'
        }
        if ($popupFrame.Right -le $popupFrame.Left -or $popupFrame.Bottom -le $popupFrame.Top) { continue }
        if (-not $owned -and $chromePopup -and
            ($popupFrame.Right -le $frame.Left -or $popupFrame.Left -ge $frame.Right -or
             $popupFrame.Bottom -le $frame.Top -or $popupFrame.Top -ge $frame.Bottom)) { continue }
        $popups.Add([ordered]@{
            hwnd = $popupHwnd.ToInt64()
            class = $class.ToString()
            frame = @($popupFrame.Left, $popupFrame.Top, ($popupFrame.Right - $popupFrame.Left), ($popupFrame.Bottom - $popupFrame.Top))
        })
    }
    return ,$popups
}

function Assert-CaptureSnapshot {
    $currentFrame = New-Object MmNative+RECT
    if ([MmNative]::DwmGetWindowAttribute($hwnd, 9, [ref]$currentFrame, 16) -ne 0) {
        throw 'Screenshot refused: windows changed during the capture.'
    }
    $snapshot = [ordered]@{
        frame = @($currentFrame.Left, $currentFrame.Top, ($currentFrame.Right - $currentFrame.Left), ($currentFrame.Bottom - $currentFrame.Top))
        popups = Get-Popups
    }
    if (($snapshot.frame -join ',') -ne ($window.frame -join ',') -or $snapshot.popups.Count -ne $popups.Count) {
        throw 'Screenshot refused: windows changed during the capture.'
    }
    foreach ($popup in $snapshot.popups) {
        $walked = @($popups | Where-Object { $_.hwnd -eq $popup.hwnd })
        if ($walked.Count -ne 1 -or ($popup.frame -join ',') -ne ($walked[0].frame -join ',')) {
            throw 'Screenshot refused: windows changed during the capture.'
        }
    }
}

$popups = Get-Popups

$shotLeft = $frame.Left; $shotTop = $frame.Top
$shotRight = $frame.Right; $shotBottom = $frame.Bottom
foreach ($popup in $popups) {
    $shotLeft = [Math]::Min($shotLeft, $popup.frame[0])
    $shotTop = [Math]::Min($shotTop, $popup.frame[1])
    $shotRight = [Math]::Max($shotRight, ($popup.frame[0] + $popup.frame[2]))
    $shotBottom = [Math]::Max($shotBottom, ($popup.frame[1] + $popup.frame[3]))
}
# 76..79 = virtual-screen x, y, width and height (including negative monitor origins).
$screenLeft = [MmNative]::GetSystemMetrics(76); $screenTop = [MmNative]::GetSystemMetrics(77)
$shotLeft = [Math]::Max($shotLeft, $screenLeft)
$shotTop = [Math]::Max($shotTop, $screenTop)
$shotRight = [Math]::Min($shotRight, ($screenLeft + [MmNative]::GetSystemMetrics(78)))
$shotBottom = [Math]::Min($shotBottom, ($screenTop + [MmNative]::GetSystemMetrics(79)))

$walker = [System.Windows.Automation.TreeWalker]::ControlViewWalker
$nodes = New-Object 'System.Collections.Generic.List[object]'
$script:walkIncomplete = $false

function Visit($element, [int]$level, $parentId, [int]$root) {
    if ($nodes.Count -ge $MaxNodes) { return }
    $c = $element.Current
    $r = $c.BoundingRectangle
    $id = 'e' + $nodes.Count
    $passwordLike = Test-PasswordLike ($c.ControlType.ProgrammaticName -replace '^ControlType\.', '') `
        ([string]$c.Name) ([string]$c.AutomationId) ([string]$c.HelpText) ([string]$c.ClassName)
    $nodes.Add([ordered]@{
        id = $id
        parent = $parentId
        root = $root
        level = $level
        type = ($c.ControlType.ProgrammaticName -replace '^ControlType\.', '')
        name = $c.Name
        automation_id = $c.AutomationId
        class = $c.ClassName
        help = $c.HelpText
        rect = $(if ($r.IsEmpty) { $null } else { @([int]$r.X, [int]$r.Y, [int]$r.Width, [int]$r.Height) })
        enabled = $c.IsEnabled
        offscreen = $c.IsOffscreen
        password = $c.IsPassword
        password_like = $passwordLike
        focusable = $c.IsKeyboardFocusable
    })
    if ($level -ge $Depth) { $script:walkIncomplete = $true; return }
    try { $child = $walker.GetFirstChild($element) } catch { $script:walkIncomplete = $true; return }
    while ($null -ne $child -and $nodes.Count -lt $MaxNodes) {
        try { Visit $child ($level + 1) $id $root } catch { $script:walkIncomplete = $true }
        try { $child = $walker.GetNextSibling($child) } catch { $script:walkIncomplete = $true; break }
    }
}

$started = Get-Date
try { Visit ([System.Windows.Automation.AutomationElement]::FromHandle($hwnd)) 0 $null 0 }
catch { throw 'Cannot walk the foreground window.' }
# All roots share the node budget and the same depth limit.
for ($i = 0; $i -lt $popups.Count; $i++) {
    if ($nodes.Count -ge $MaxNodes) { $script:walkIncomplete = $true; break }
    try { Visit ([System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]($popups[$i].hwnd))) 0 $null ($i + 1) }
    catch { $script:walkIncomplete = $true }
}
$ms = [int]((Get-Date) - $started).TotalMilliseconds

$window = [ordered]@{
    title = $title.ToString()
    process = $procName
    frame = @($frame.Left, $frame.Top, ($frame.Right - $frame.Left), ($frame.Bottom - $frame.Top))
    popups = @($popups.ToArray())
    shot_frame = @($shotLeft, $shotTop, [Math]::Max(0, ($shotRight - $shotLeft)), [Math]::Max(0, ($shotBottom - $shotTop)))
    dpi = [MmNative]::GetDpiForWindow($hwnd)
    dpi_awareness = $awareness
    captured_at = (Get-Date).ToString('s')
    walk_ms = $ms
    truncated = ($nodes.Count -ge $MaxNodes -or $script:walkIncomplete)
}

Write-Output ("{0} [{1}]  frame {2}  dpi {3} ({4})  {5} nodes in {6} ms{7}" -f $window.title, $procName,
    ($window.frame -join ','), $window.dpi, $awareness, $nodes.Count, $ms, $(if ($window.truncated) { ' (truncated)' } else { '' }))
for ($i = 0; $i -lt $popups.Count; $i++) {
    Write-Output ('Popup {0} [{1}] hwnd {2}  frame {3}' -f ($i + 1), $popups[$i].class, $popups[$i].hwnd, ($popups[$i].frame -join ','))
}
foreach ($n in $nodes | Select-Object -First 250) {
    $rect = if ($n.rect) { '[' + ($n.rect -join ',') + ']' } else { '[-]' }
    $flags = @()
    if (-not $n.enabled) { $flags += 'disabled' }
    if ($n.offscreen) { $flags += 'offscreen' }
    if ($n.password) { $flags += 'PASSWORD' }
    if ($n.password_like) { $flags += 'PASSWORD?' }
    $extra = if ($flags) { ' {' + ($flags -join ',') + '}' } else { '' }
    Write-Output ('{0}{1} {2} "{3}" {4}{5}' -f ('  ' * $n.level), $n.id, $n.type, $n.name, $rect, $extra)
}
if ($nodes.Count -gt 250) { Write-Output "... $($nodes.Count - 250) more nodes (see the JSON)" }

if ($JsonPath) {
    $json = [ordered]@{ window = $window; nodes = $nodes } | ConvertTo-Json -Depth 6
    [System.IO.File]::WriteAllText($JsonPath, $json, (New-Object System.Text.UTF8Encoding $false))
}
if ($ShotPath) {
    # Fail closed: this development tool does not implement password redaction.
    if ($aw -ne 2 -or $window.truncated) {
        throw 'Screenshot refused: DPI or password safety cannot be guaranteed.'
    }
    if (@($nodes | Where-Object { $_.password -or $_.password_like }).Count -gt 0) {
        throw 'Screenshot refused: a password-like field is on screen.'
    }
    $w = $window.shot_frame[2]; $h = $window.shot_frame[3]
    if ($w -gt 0 -and $h -gt 0) {
        $bmp = New-Object System.Drawing.Bitmap $w, $h
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        try {
            # Keep gaps between walked frames black; only copy their visible screen regions.
            $g.Clear([System.Drawing.Color]::Black)
            $frames = @(,$window.frame) + @($popups | ForEach-Object { ,$_.frame })
            Assert-CaptureSnapshot
            foreach ($captureFrame in $frames) {
                $left = [Math]::Max($captureFrame[0], $shotLeft)
                $top = [Math]::Max($captureFrame[1], $shotTop)
                $right = [Math]::Min(($captureFrame[0] + $captureFrame[2]), $shotRight)
                $bottom = [Math]::Min(($captureFrame[1] + $captureFrame[3]), $shotBottom)
                if ($right -le $left -or $bottom -le $top) { continue }
                $size = New-Object System.Drawing.Size ($right - $left), ($bottom - $top)
                $g.CopyFromScreen($left, $top, ($left - $shotLeft), ($top - $shotTop), $size)
            }
            Assert-CaptureSnapshot
            $bmp.Save($ShotPath, [System.Drawing.Imaging.ImageFormat]::Png)
        } finally { $g.Dispose(); $bmp.Dispose() }
    }
}
