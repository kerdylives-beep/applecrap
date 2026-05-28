param(
  [Parameter(Mandatory = $true)]
  [string]$Operation,
  [string]$TrackUrl = '',
  [string]$TrackTitle = '',
  [string]$TrackArtist = ''
)

$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type @"
using System;
using System.Runtime.InteropServices;

public static class AppleCrapNative {
  [DllImport("user32.dll")]
  public static extern bool SetForegroundWindow(IntPtr hWnd);

  [DllImport("user32.dll")]
  public static extern bool ShowWindowAsync(IntPtr hWnd, int nCmdShow);

  [DllImport("user32.dll")]
  public static extern IntPtr GetForegroundWindow();

  [DllImport("user32.dll")]
  public static extern bool IsWindow(IntPtr hWnd);

  [DllImport("user32.dll")]
  public static extern void keybd_event(byte bVk, byte bScan, int dwFlags, int dwExtraInfo);
}
"@

function Emit-Result($ok, $summary, $detail) {
  [pscustomobject]@{
    ok = $ok
    summary = $summary
    detail = $detail
  } | ConvertTo-Json -Compress
}

function Get-AppleMusicProcess {
  Get-Process AppleMusic -ErrorAction SilentlyContinue |
    Where-Object { $_.MainWindowHandle -ne 0 } |
    Select-Object -First 1
}

function Capture-ForegroundWindowHandle {
  return [AppleCrapNative]::GetForegroundWindow()
}

function Restore-ForegroundWindow {
  param(
    [System.IntPtr]$Handle,
    [System.Diagnostics.Process]$AppleMusicProcess = $null,
    [int]$DelayMs = 140,
    [int]$Attempts = 4
  )

  if ($Handle -eq [System.IntPtr]::Zero) {
    return $false
  }

  if (-not [AppleCrapNative]::IsWindow($Handle)) {
    return $false
  }

  if ($AppleMusicProcess -and $Handle -eq $AppleMusicProcess.MainWindowHandle) {
    return $false
  }

  Start-Sleep -Milliseconds $DelayMs

  for ($attempt = 0; $attempt -lt $Attempts; $attempt++) {
    [AppleCrapNative]::ShowWindowAsync($Handle, 9) | Out-Null
    [AppleCrapNative]::SetForegroundWindow($Handle) | Out-Null
    Start-Sleep -Milliseconds 35

    if ([AppleCrapNative]::GetForegroundWindow() -eq $Handle) {
      return $true
    }
  }

  return $false
}

function Focus-AppleMusicWindow {
  param(
    [int]$Attempts = 6
  )

  for ($attempt = 0; $attempt -lt $Attempts; $attempt++) {
    $process = Get-AppleMusicProcess
    if ($process) {
      [AppleCrapNative]::ShowWindowAsync($process.MainWindowHandle, 9) | Out-Null
      [AppleCrapNative]::SetForegroundWindow($process.MainWindowHandle) | Out-Null
      Start-Sleep -Milliseconds 55
      return $process
    }

    Start-Sleep -Milliseconds 110
  }

  return $null
}

function Get-AppleMusicRoot($process) {
  if (-not $process) {
    return $null
  }

  return [System.Windows.Automation.AutomationElement]::FromHandle($process.MainWindowHandle)
}

function New-ControlTypeCondition($controlType) {
  return New-Object System.Windows.Automation.PropertyCondition(
    [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
    $controlType
  )
}

function Find-VisiblePlayButtons($root) {
  $all = $root.FindAll(
    [System.Windows.Automation.TreeScope]::Descendants,
    (New-ControlTypeCondition ([System.Windows.Automation.ControlType]::Button))
  )
  $matches = @()

  foreach ($element in $all) {
    if (
      $element.Current.ControlType.ProgrammaticName -eq 'ControlType.Button' -and
      $element.Current.Name -eq 'Play' -and
      $element.Current.IsEnabled -and
      -not $element.Current.IsOffscreen
    ) {
      $matches += $element
    }
  }

  return $matches | Sort-Object {
    if ($_.Current.BoundingRectangle.Y -lt 0) { 99999 } else { $_.Current.BoundingRectangle.Y }
  }, {
    if ($_.Current.BoundingRectangle.X -lt 0) { 99999 } else { $_.Current.BoundingRectangle.X }
  }
}

function Normalize-ComparableText($value) {
  if (-not $value) {
    return ''
  }

  return (($value.ToLowerInvariant().ToCharArray() | ForEach-Object {
    if ([char]::IsLetterOrDigit($_)) { $_ } else { ' ' }
  }) -join '') -replace '\s+', ' ' -replace '^\s+|\s+$', ''
}

function Find-TrackRow($root, $trackTitle) {
  $needle = Normalize-ComparableText $trackTitle
  if (-not $needle) {
    return $null
  }

  $all = $root.FindAll(
    [System.Windows.Automation.TreeScope]::Descendants,
    (New-ControlTypeCondition ([System.Windows.Automation.ControlType]::ListItem))
  )

  $candidates = @()
  foreach ($element in $all) {
    if (
      $element.Current.ControlType.ProgrammaticName -ne 'ControlType.ListItem' -or
      $element.Current.IsOffscreen -or
      -not $element.Current.Name -or
      $element.Current.Name -notlike 'Track *'
    ) {
      continue
    }

    $normalizedName = Normalize-ComparableText $element.Current.Name
    if ($normalizedName -like "*$needle*") {
      $candidates += $element
    }
  }

  return $candidates | Sort-Object {
    if ($_.Current.BoundingRectangle.Y -lt 0) { 99999 } else { $_.Current.BoundingRectangle.Y }
  }, {
    if ($_.Current.BoundingRectangle.X -lt 0) { 99999 } else { $_.Current.BoundingRectangle.X }
  } | Select-Object -First 1
}

function Wait-ForTrackRow {
  param(
    $process,
    [string]$TrackTitle,
    [int]$Attempts = 8,
    [int]$DelayMs = 45
  )

  if (-not $TrackTitle) {
    return $null
  }

  for ($attempt = 0; $attempt -lt $Attempts; $attempt++) {
    $root = Get-AppleMusicRoot $process
    if ($root) {
      $trackRow = Find-TrackRow $root $TrackTitle
      if ($trackRow) {
        return [pscustomobject]@{
          Root = $root
          Row = $trackRow
        }
      }
    }

    Start-Sleep -Milliseconds $DelayMs
  }

  return $null
}

function Find-MenuItemByName {
  param(
    [string[]]$CandidateNames,
    [int]$Attempts = 4,
    [int]$DelayMs = 25
  )

  if (-not $CandidateNames -or $CandidateNames.Count -eq 0) {
    return $null
  }

  $menuCondition = New-ControlTypeCondition ([System.Windows.Automation.ControlType]::Menu)
  $menuItemCondition = New-ControlTypeCondition ([System.Windows.Automation.ControlType]::MenuItem)
  $normalizedCandidates = $CandidateNames |
    Where-Object { $_ } |
    ForEach-Object { Normalize-ComparableText $_ } |
    Where-Object { $_ }

  for ($attempt = 0; $attempt -lt $Attempts; $attempt++) {
    $root = [System.Windows.Automation.AutomationElement]::RootElement
    $menus = $root.FindAll(
      [System.Windows.Automation.TreeScope]::Descendants,
      $menuCondition
    )

    foreach ($menu in $menus) {
      if ($menu.Current.IsOffscreen) {
        continue
      }

      $items = $menu.FindAll(
        [System.Windows.Automation.TreeScope]::Descendants,
        $menuItemCondition
      )

      foreach ($element in $items) {
        if ($element.Current.IsOffscreen) {
          continue
        }

        $normalizedName = Normalize-ComparableText $element.Current.Name
        if ($normalizedCandidates -contains $normalizedName) {
          return $element
        }
      }
    }

    Start-Sleep -Milliseconds $DelayMs
  }

  return $null
}

function Find-RowMoreButton($trackRow) {
  $children = $trackRow.FindAll(
    [System.Windows.Automation.TreeScope]::Descendants,
    (New-ControlTypeCondition ([System.Windows.Automation.ControlType]::Button))
  )

  foreach ($child in $children) {
    if (
      $child.Current.ControlType.ProgrammaticName -eq 'ControlType.Button' -and
      $child.Current.Name -eq 'More' -and
      -not $child.Current.IsOffscreen
    ) {
      return $child
    }
  }

  return $null
}

function Find-TrackPlayMenuItem {
  param(
    [string]$TrackTitle,
    [string]$ActionPrefix = 'play',
    [switch]$IgnoreTrackTitle,
    [int]$Attempts = 3
  )

  $needle = Normalize-ComparableText $TrackTitle
  $normalizedActionPrefix = Normalize-ComparableText $ActionPrefix
  if (-not $normalizedActionPrefix) {
    return $null
  }

  if ($IgnoreTrackTitle -and $normalizedActionPrefix -eq 'play next') {
    $directPlayNext = Find-MenuItemByName -CandidateNames @('Play Next') -Attempts 3 -DelayMs 20
    if ($directPlayNext) {
      return $directPlayNext
    }
  }

  $menuCondition = New-ControlTypeCondition ([System.Windows.Automation.ControlType]::Menu)
  $menuItemCondition = New-ControlTypeCondition ([System.Windows.Automation.ControlType]::MenuItem)
  for ($attempt = 0; $attempt -lt $attempts; $attempt++) {
    $root = [System.Windows.Automation.AutomationElement]::RootElement
    $menus = $root.FindAll(
      [System.Windows.Automation.TreeScope]::Descendants,
      $menuCondition
    )

    $exactMatches = @()
    $partialMatches = @()

    foreach ($menu in $menus) {
      if ($menu.Current.IsOffscreen) {
        continue
      }

      $items = $menu.FindAll(
        [System.Windows.Automation.TreeScope]::Descendants,
        $menuItemCondition
      )

      foreach ($element in $items) {
        if (
          $element.Current.ControlType.ProgrammaticName -eq 'ControlType.MenuItem' -and
          -not $element.Current.IsOffscreen
        ) {
          $normalizedName = Normalize-ComparableText $element.Current.Name
          if ($normalizedName -like "$normalizedActionPrefix*") {
            if ($IgnoreTrackTitle -or -not $needle) {
              return $element
            }

            $suffix = $normalizedName.Substring($normalizedActionPrefix.Length).Trim()
            if ($suffix -eq $needle) {
              $exactMatches += $element
              continue
            }

            if ($suffix -like "*$needle*") {
              $partialMatches += $element
            }
          }
        }
      }
    }

    if ($exactMatches.Count -gt 0) {
      return $exactMatches | Select-Object -First 1
    }

    if ($partialMatches.Count -gt 0) {
      return $partialMatches | Sort-Object {
        (Normalize-ComparableText $_.Current.Name).Length
      } | Select-Object -First 1
    }

    Start-Sleep -Milliseconds 20
  }

  return $null
}

function Invoke-PlayButton($button) {
  try {
    $invokePattern = $button.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
    if ($invokePattern) {
      ([System.Windows.Automation.InvokePattern]$invokePattern).Invoke()
      return $true
    }
  } catch {}

  try {
    $selectionPattern = $button.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern)
    if ($selectionPattern) {
      ([System.Windows.Automation.SelectionItemPattern]$selectionPattern).Select()
      return $true
    }
  } catch {}

  try {
    $legacyPattern = $button.GetCurrentPattern([System.Windows.Automation.LegacyIAccessiblePattern]::Pattern)
    if ($legacyPattern) {
      ([System.Windows.Automation.LegacyIAccessiblePattern]$legacyPattern).DoDefaultAction()
      return $true
    }
  } catch {}

  return $false
}

function Send-MediaPlayPause {
  [AppleCrapNative]::keybd_event(0xB3, 0, 0, 0)
  Start-Sleep -Milliseconds 80
  [AppleCrapNative]::keybd_event(0xB3, 0, 2, 0)
}

try {
  switch ($Operation) {
    'probe-capabilities' {
      Emit-Result $true 'UI automation adapter is reachable.' 'The experimental adapter can launch Apple Music, open a track URL, and attempt a best-effort play button invocation. Queue mutation remains best-effort only.'
      break
    }
    'focus-player' {
      Start-Process 'shell:AppsFolder\AppleInc.AppleMusicWin_nzyj5cx40ttqa!App'
      Emit-Result $true 'Apple Music focus requested.' 'The experimental adapter launched or focused Apple Music.'
      break
    }
    'open-track' {
      if (-not $TrackUrl) {
        Emit-Result $false 'No track URL supplied.' 'Open-track requires a resolved Apple Music URL or search URL.'
        break
      }

      $previousWindow = Capture-ForegroundWindowHandle
      Start-Process $TrackUrl
      $process = Focus-AppleMusicWindow
      Restore-ForegroundWindow -Handle $previousWindow -AppleMusicProcess $process | Out-Null
      Emit-Result $true 'Track open requested.' "The experimental adapter opened $TrackUrl"
      break
    }
    'attempt-play' {
      $previousWindow = Capture-ForegroundWindowHandle
      if ($TrackUrl) {
        Start-Process $TrackUrl
        Start-Sleep -Milliseconds 120
      }

      $process = Focus-AppleMusicWindow
      if (-not $process) {
        Emit-Result $false 'Apple Music window not found.' 'Apple Music was not exposing a foregroundable window for the experimental play attempt.'
        break
      }

      $root = Get-AppleMusicRoot $process
      $attemptedExactTrackPlay = $false
      if ($TrackTitle) {
        $trackWait = Wait-ForTrackRow -process $process -TrackTitle $TrackTitle
        $trackRow = $null
        if ($trackWait) {
          $trackRow = $trackWait.Row
          $root = $trackWait.Root
        }

        if ($trackRow) {
            $attemptedExactTrackPlay = $true
            $moreButton = Find-RowMoreButton $trackRow
          if ($moreButton -and (Invoke-PlayButton $moreButton)) {
            $playItem = Find-TrackPlayMenuItem -TrackTitle $TrackTitle -ActionPrefix 'play'
            if ($playItem -and (Invoke-PlayButton $playItem)) {
              Restore-ForegroundWindow -Handle $previousWindow -AppleMusicProcess $process | Out-Null
              Emit-Result $true 'Exact track play invoked.' "The experimental adapter opened the selected target and invoked Play for `"$TrackTitle`"."
              break
            }

            Emit-Result $false 'Exact track play action unavailable.' "Apple Music exposed a row for `"$TrackTitle`", but it did not expose an exact Play action for that song."
            break
          }

          Emit-Result $false 'Track actions menu unavailable.' "The adapter found a visible row for `"$TrackTitle`", but could not open its track actions menu."
          break
        }
      }

      if ($attemptedExactTrackPlay) {
        Emit-Result $false 'Exact track play could not be confirmed.' "The adapter refused to fall back to a generic Play button because the exact song could not be proven."
        break
      }

      $buttons = Find-VisiblePlayButtons $root
      $invoked = $false

      foreach ($button in $buttons) {
        if (Invoke-PlayButton $button) {
          $invoked = $true
          break
        }
      }

      if ($invoked) {
        Restore-ForegroundWindow -Handle $previousWindow -AppleMusicProcess $process | Out-Null
        Emit-Result $true 'Play button invoked.' 'The experimental adapter opened the selected target, focused Apple Music, and invoked a visible Play button.'
        break
      }

      Send-MediaPlayPause
      Restore-ForegroundWindow -Handle $previousWindow -AppleMusicProcess $process | Out-Null
      Emit-Result $true 'Media play command sent.' 'The experimental adapter could not invoke a visible Play button, so it sent a best-effort media play command to Apple Music.'
      break
    }
    'attempt-queue-action' {
      $previousWindow = Capture-ForegroundWindowHandle
      if ($TrackUrl) {
        Start-Process $TrackUrl
        Start-Sleep -Milliseconds 120
      }

      $process = Focus-AppleMusicWindow
      if (-not $process) {
        Emit-Result $false 'Apple Music window not found.' 'Apple Music was not exposing a foregroundable window for the Play Next attempt.'
        break
      }

      $root = Get-AppleMusicRoot $process
      $trackRow = $null
      if ($TrackTitle) {
        $trackWait = Wait-ForTrackRow -process $process -TrackTitle $TrackTitle
        if ($trackWait) {
          $trackRow = $trackWait.Row
          $root = $trackWait.Root
        }
      }

      if (-not $trackRow) {
        Emit-Result $false 'Track row not found.' "The adapter could not locate the visible row for `"$TrackTitle`" to queue it next."
        break
      }

      $moreButton = Find-RowMoreButton $trackRow
      if (-not $moreButton -or -not (Invoke-PlayButton $moreButton)) {
        Emit-Result $false 'Track actions menu unavailable.' "The adapter could not open the track actions menu for `"$TrackTitle`"."
        break
      }

      $playNextItem = Find-TrackPlayMenuItem -TrackTitle $TrackTitle -ActionPrefix 'play next' -IgnoreTrackTitle
      if (-not $playNextItem -or -not (Invoke-PlayButton $playNextItem)) {
        Emit-Result $false 'Play Next action unavailable.' "Apple Music did not expose a Play Next action for `"$TrackTitle`"."
        break
      }

      Restore-ForegroundWindow -Handle $previousWindow -AppleMusicProcess $process | Out-Null
      Emit-Result $true 'Queued as Play Next.' "The experimental adapter opened the selected target and queued `"$TrackTitle`" to play next."
      break
    }
    'dry-run' {
      Emit-Result $true 'Dry run complete.' 'No player actions were taken. The adapter boundary is reachable and would not mutate the queue.'
      break
    }
    default {
      Emit-Result $false 'Unknown automation action.' "Unsupported operation: $Operation"
    }
  }
} catch {
  Emit-Result $false 'Automation script failed.' $_.Exception.Message
}
