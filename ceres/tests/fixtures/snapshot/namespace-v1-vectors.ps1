# Independent .NET framing/SHA-256 oracle. Emits JSON only; never calls Rust.
# Run in PowerShell 7. Do not regenerate expected vectors using the codec under test.
$ErrorActionPreference = 'Stop'
function New-Bytes([string]$domain) {
    $buffer = [System.Collections.Generic.List[byte]]::new()
    $buffer.AddRange([System.Text.Encoding]::UTF8.GetBytes($domain))
    $buffer.Add(0)
    return ,$buffer
}
function Add-Field($buffer, [byte[]]$bytes) {
    $length = [System.BitConverter]::GetBytes([uint32]$bytes.Length)
    if ([System.BitConverter]::IsLittleEndian) { [array]::Reverse($length) }
    $buffer.AddRange($length)
    $buffer.AddRange($bytes)
}
function Add-Text($buffer, [string]$value) {
    Add-Field $buffer ([System.Text.Encoding]::UTF8.GetBytes($value))
}
function Source-Bytes($source) {
    $buffer = New-Bytes 'mega.source-snapshot.v1'
    foreach ($field in @('source_id','scope_path','object_format','commit_oid','root_tree_oid')) {
        Add-Text $buffer $source[$field]
    }
    return ,$buffer.ToArray()
}
function Digest([byte[]]$bytes) {
    return 'sha256:' + [System.Convert]::ToHexString([System.Security.Cryptography.SHA256]::HashData($bytes)).ToLowerInvariant()
}
function Hex([byte[]]$bytes) { return [System.Convert]::ToHexString($bytes).ToLowerInvariant() }
$native = [ordered]@{
    source_id='11111111-1111-4111-8111-111111111111'; scope_path='/'; object_format='sha1'
    commit_oid=('1' * 40); root_tree_oid='4b825dc642cb6eb9a060e54bf8d69288fbee4904'
}
$import = [ordered]@{
    source_id='33333333-3333-4333-8333-333333333333'; scope_path='/third-party/库+1'; object_format='sha1'
    commit_oid=('a' * 40); root_tree_oid=('b' * 40)
}
$bindings = @()
foreach ($policy in @('mutable','immutable_release')) {
    $binding = [ordered]@{
        mount_path='/deps/库+1'; source_snapshot=$import; source_subpath='src'; policy=$policy
    }
    $buffer = New-Bytes 'mega.namespace-binding.v1'
    Add-Text $buffer $binding.mount_path
    Add-Field $buffer (Source-Bytes $import)
    Add-Text $buffer $binding.source_subpath
    if ($policy -eq 'mutable') { $buffer.Add(1) } else { $buffer.Add(2) }
    $bindings += [ordered]@{binding=$binding; canonical_hex=(Hex $buffer.ToArray()); digest=(Digest $buffer.ToArray())}
}
$empty = 'sha256:18946486089198dfa8eeb70fa90e04b137c579dc08ae1e6f8bceafc0d35ef677'
$views = @()
foreach ($overrides in @($null, ('sha256:' + ('c' * 64)))) {
    $view = [ordered]@{
        schema_version=1; instance_id='22222222-2222-4222-8222-222222222222'; native=$native
        bindings_root=$empty; overrides_root=$overrides; materialization_policy='git_raw_v1'
    }
    $buffer = New-Bytes 'mega.namespace-view.v1'
    $buffer.AddRange([byte[]]@(0,1))
    Add-Text $buffer $view.instance_id
    Add-Field $buffer (Source-Bytes $native)
    $buffer.AddRange([System.Convert]::FromHexString($empty.Substring(7)))
    if ($null -eq $overrides) { $buffer.Add(0) } else {
        $buffer.Add(1)
        $buffer.AddRange([System.Convert]::FromHexString($overrides.Substring(7)))
    }
    $buffer.Add(1)
    $views += [ordered]@{view=$view; canonical_hex=(Hex $buffer.ToArray()); digest=(Digest $buffer.ToArray())}
}
[ordered]@{bindings=$bindings; views=$views} | ConvertTo-Json -Depth 10
