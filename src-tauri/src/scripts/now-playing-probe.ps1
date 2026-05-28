$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

try {
  Add-Type -AssemblyName UIAutomationClient
  Add-Type -AssemblyName UIAutomationTypes

  function Get-DescendantSnapshots($root) {
    $all = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
    $items = @()
    foreach ($element in $all) {
      $items += [pscustomobject]@{
        controlType = $element.Current.ControlType.ProgrammaticName
        name = $element.Current.Name
      }
    }
    return $items
  }

  function Get-PlaybackStatus($items) {
    if ($items | Where-Object { $_.controlType -eq 'ControlType.Button' -and $_.name -eq 'Pause' }) {
      return 'Playing'
    }
    if ($items | Where-Object { $_.controlType -eq 'ControlType.Button' -and $_.name -eq 'Play' }) {
      return 'Paused'
    }
    return 'Unknown'
  }

  function Get-SpotifySession {
    $process = Get-Process Spotify -ErrorAction SilentlyContinue |
      Where-Object { $_.MainWindowHandle -ne 0 -and $_.MainWindowTitle } |
      Select-Object -First 1

    if (-not $process) {
      return $null
    }

    $root = [System.Windows.Automation.AutomationElement]::FromHandle($process.MainWindowHandle)
    $items = Get-DescendantSnapshots $root
    $nowPlaying = $items | Where-Object { $_.name -like 'Now playing:* by *' } | Select-Object -First 1

    $title = ''
    $artist = ''
    if ($nowPlaying -and $nowPlaying.name -match '^Now playing:\s*(.+?)\s+by\s+(.+)$') {
      $title = $matches[1].Trim()
      $artist = $matches[2].Trim()
    } elseif ($process.MainWindowTitle -match '^(.*?)\s+-\s+(.*)$') {
      $artist = $matches[1].Trim()
      $title = $matches[2].Trim()
    }

    if (-not $title) {
      return $null
    }

    return [pscustomobject]@{
      appId = 'Spotify'
      status = Get-PlaybackStatus $items
      title = $title
      artist = $artist
      album = ''
    }
  }

  function Get-MediaPlayerSession {
    $process = Get-Process ApplicationFrameHost -ErrorAction SilentlyContinue |
      Where-Object { $_.MainWindowHandle -ne 0 -and $_.MainWindowTitle -eq 'Media Player' } |
      Select-Object -First 1

    if (-not $process) {
      return $null
    }

    $root = [System.Windows.Automation.AutomationElement]::FromHandle($process.MainWindowHandle)
    $items = Get-DescendantSnapshots $root

    $nowPlayingIndex = -1
    for ($index = 0; $index -lt $items.Count; $index++) {
      if ($items[$index].name -like '*. Now playing.' -or $items[$index].name -like '*. Now playing') {
        $nowPlayingIndex = $index
        break
      }
    }

    if ($nowPlayingIndex -lt 0) {
      return $null
    }

    $textItems = @()
    for ($index = $nowPlayingIndex + 1; $index -lt [Math]::Min($items.Count, $nowPlayingIndex + 10); $index++) {
      if ($items[$index].controlType -eq 'ControlType.Text' -and $items[$index].name) {
        $textItems += $items[$index].name
      }
    }

    if ($textItems.Count -eq 0) {
      return $null
    }

    return [pscustomobject]@{
      appId = 'Media Player'
      status = Get-PlaybackStatus $items
      title = $textItems[0]
      artist = if ($textItems.Count -ge 2) { $textItems[1] } else { '' }
      album = ''
    }
  }

  function Get-AppleMusicSession {
    $process = Get-Process AppleMusic -ErrorAction SilentlyContinue |
      Where-Object { $_.MainWindowHandle -ne 0 } |
      Select-Object -First 1

    if (-not $process) {
      return $null
    }

    $root = [System.Windows.Automation.AutomationElement]::FromHandle($process.MainWindowHandle)
    $items = Get-DescendantSnapshots $root
    $status = Get-PlaybackStatus $items
    $dash = [string][char]0x2014

    $groups = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
    foreach ($group in $groups) {
      if ($group.Current.ControlType.ProgrammaticName -ne 'ControlType.Group') {
        continue
      }

      $children = $group.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
      $textItems = @()
      foreach ($child in $children) {
        if ($child.Current.ControlType.ProgrammaticName -eq 'ControlType.Text' -and $child.Current.Name) {
          $textItems += $child.Current.Name
        }
      }

      if ($textItems.Count -lt 2) {
        continue
      }

      $title = $textItems[0].Trim()
      $artist = ''
      $album = ''
      $meta = $textItems[1] -replace $dash, ' - '
      if ($meta -match '^(.*?)\s+-\s+(.+)$') {
        $artist = $matches[1].Trim()
        $album = $matches[2].Trim()
      }

      if ($title -and $artist) {
        return [pscustomobject]@{
          appId = 'Apple Music'
          status = $status
          title = $title
          artist = $artist
          album = $album
        }
      }
    }

    return $null
  }

  $sessions = @()
  foreach ($detector in @('Get-SpotifySession', 'Get-MediaPlayerSession', 'Get-AppleMusicSession')) {
    $session = & $detector
    if ($session) {
      $sessions += $session
    }
  }

  $chosen = $sessions | Where-Object { $_.appId -eq 'Apple Music' -and $_.status -eq 'Playing' } | Select-Object -First 1
  if (-not $chosen) {
    $chosen = $sessions | Where-Object { $_.status -eq 'Playing' } | Select-Object -First 1
  }
  if (-not $chosen) {
    $chosen = $sessions | Select-Object -First 1
  }

  [pscustomobject]@{
    session = $chosen
    sessions = $sessions
    error = $null
  } | ConvertTo-Json -Depth 6 -Compress
} catch {
  [pscustomobject]@{
    session = $null
    sessions = @()
    error = $_.Exception.Message
  } | ConvertTo-Json -Depth 6 -Compress
}
