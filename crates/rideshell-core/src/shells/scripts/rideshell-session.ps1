
# ----- @CMD@ window: run cmd and bash command lines automatically -------------
# Each line is checked before it runs. PowerShell lines run normally; lines
# that only make sense in cmd or bash run there, and their directory and
# environment changes come back to this session. Prefixes force a shell:
# @cmd, @bash, @ps.

$global:@FN@_auto = @AUTO_SWITCH@
$global:@FN@_bash = @BASH_AVAILABLE@

function global:@FN@_hints([string]$line) {
    $tokens = $null
    $errors = $null
    $ast = [System.Management.Automation.Language.Parser]::ParseInput($line, [ref]$tokens, [ref]$errors)
    if (@($errors | Where-Object { $_.IncompleteInput }).Count -gt 0) { return $null }
    $kinds = New-Object System.Collections.Generic.List[string]
    $pure = $true
    $commands = $ast.FindAll({ param($n) $n -is [System.Management.Automation.Language.CommandAst] }, $true)
    foreach ($c in $commands) {
        $name = $c.GetCommandName()
        if (-not $name) { continue }
        $found = Get-Command -Name $name -ErrorAction SilentlyContinue | Select-Object -First 1
        $kind = if ($found) { $found.CommandType.ToString() } else { 'Missing' }
        if ($kind -ne 'Cmdlet' -and $kind -ne 'Function' -and $kind -ne 'ExternalScript') { $pure = $false }
        $kinds.Add("$name=$kind")
    }
    [pscustomobject]@{ Errors = $errors.Count; Commands = ($kinds -join ';'); Pure = ($pure -and $errors.Count -eq 0) }
}

# Which shell should run $line? 'pwsh', 'cmd', 'bash', or 'incomplete'.
function global:@FN@_route([string]$line) {
    if ($line -match '^\s*@(cmd|bash|sh|ps|pwsh|powershell)(\s|$)') {
        $d = $Matches[1]
        if ($d -eq 'sh') { return 'bash' }
        if ($d -eq 'ps' -or $d -eq 'powershell') { return 'pwsh' }
        return $d
    }
    if (-not $global:@FN@_auto -or -not $line.Trim()) { return 'pwsh' }
    $h = @FN@_hints $line
    if ($null -eq $h) { return 'incomplete' }
    # Fast path: only cmdlets/functions and no parse errors is plain PowerShell.
    if ($h.Pure -and $line -notmatch '%\w') { return 'pwsh' }
    $saved = $global:LASTEXITCODE
    $env:@ENV_LINE@ = $line
    $env:@ENV_PS_ERRORS@ = [string]$h.Errors
    $env:@ENV_PS_COMMANDS@ = $h.Commands
    try {
        $d = (& $global:@FN@_bin route) -join ''
    } finally {
        Remove-Item Env:\@ENV_LINE@, Env:\@ENV_PS_ERRORS@, Env:\@ENV_PS_COMMANDS@ -ErrorAction SilentlyContinue
        $global:LASTEXITCODE = $saved
    }
    if ($d -eq 'cmd' -or $d -eq 'bash') { return $d }
    return 'pwsh'
}

# Run $line in cmd or bash and bring its directory/environment changes back.
function global:@FN@_run([string]$dialect, [string]$line) {
    $out = Join-Path ([IO.Path]::GetTempPath()) ("@CMD@-" + [guid]::NewGuid().ToString('N') + '.txt')
    $env:@ENV_LINE@ = $line
    $treat = [Console]::TreatControlCAsInput
    [Console]::TreatControlCAsInput = $false
    try {
        & $global:@FN@_bin dialect-exec $dialect --out $out --console
        $status = $LASTEXITCODE
    } finally {
        [Console]::TreatControlCAsInput = $treat
        Remove-Item Env:\@ENV_LINE@ -ErrorAction SilentlyContinue
    }
    if (Test-Path -LiteralPath $out) {
        $code = [IO.File]::ReadAllText($out)
        Remove-Item -LiteralPath $out -Force -ErrorAction SilentlyContinue
        if ($code.StartsWith('@MARKER@')) { Invoke-Expression $code }
    }
    @FN@_sync -Force
    $global:LASTEXITCODE = $status
}

if (Get-Command Set-PSReadLineKeyHandler -ErrorAction SilentlyContinue) {
    Set-PSReadLineKeyHandler -Key Enter -BriefDescription '@CMD@AcceptLine' `
        -Description 'Run the line in PowerShell, cmd or bash, whichever it is written for' -ScriptBlock {
        $line = $null
        $cursor = $null
        [Microsoft.PowerShell.PSConsoleReadLine]::GetBufferState([ref]$line, [ref]$cursor)
        $d = 'pwsh'
        try { $d = @FN@_route $line } catch { $d = 'pwsh' }
        if ($d -eq 'cmd' -or $d -eq 'bash') {
            [Microsoft.PowerShell.PSConsoleReadLine]::AddToHistory($line)
            [Microsoft.PowerShell.PSConsoleReadLine]::EndOfLine($null, $null)
            [Console]::WriteLine()
            try { @FN@_run $d $line } catch { Write-Warning "@CMD@: $_" }
            # New prompt below the output, with an empty line buffer.
            [Microsoft.PowerShell.PSConsoleReadLine]::InvokePrompt($null, [Console]::CursorTop)
            [Microsoft.PowerShell.PSConsoleReadLine]::GetBufferState([ref]$line, [ref]$cursor)
            if ($line.Length -gt 0) { [Microsoft.PowerShell.PSConsoleReadLine]::Replace(0, $line.Length, '') }
            return
        }
        if ($line -match '^\s*@(ps|pwsh|powershell)\s+') {
            [Microsoft.PowerShell.PSConsoleReadLine]::Replace(0, $line.Length, ($line -replace '^\s*@(ps|pwsh|powershell)\s+', ''))
        }
        [Microsoft.PowerShell.PSConsoleReadLine]::AcceptLine($null, $null)
    }
}

# Run a start-up command (`rideshell open --command`), routed like a typed line.
function global:@FN@_start([string]$line) {
    $d = 'pwsh'
    try { $d = @FN@_route $line } catch { }
    try { [Microsoft.PowerShell.PSConsoleReadLine]::AddToHistory($line) } catch { }
    if ($d -eq 'cmd' -or $d -eq 'bash') { @FN@_run $d $line } else { Invoke-Expression $line }
}

# Inside RideShell App: report profile and folder with each prompt (an
# invisible OSC sequence the app reads to label tabs).
if ($env:TERM_PROGRAM -eq '@APP@') {
    $global:@FN@_app_prompt = $function:prompt
    function global:prompt {
        $p = & $global:@FN@_app_prompt
        $loc = $executionContext.SessionState.Path.CurrentLocation.ProviderPath
        $admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
        return ([char]27 + "]7777;profile=" + $env:@ENV_ACTIVE@ + ";admin=" + [int]$admin + ";cwd=" + $loc + [char]7) + $p
    }
}

# `sudo <command>`: run one command in an Administrator window (unless a
# real sudo is installed).
if (-not (Get-Command sudo -ErrorAction SilentlyContinue)) {
    function global:sudo { & $global:@FN@_bin admin -- @args }
}

if ($env:TERM_PROGRAM -ne '@APP@') { $Host.UI.RawUI.WindowTitle = '@APP@' }
