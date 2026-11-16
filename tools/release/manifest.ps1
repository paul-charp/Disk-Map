# Writes an updater manifest (latest.json format) for one signed NSIS installer.
param(
    [Parameter(Mandatory)] [string] $Version,
    [Parameter(Mandatory)] [string] $Notes,
    [Parameter(Mandatory)] [string] $Url,
    [Parameter(Mandatory)] [string] $Sig,
    [Parameter(Mandatory)] [string] $Out
)

$manifest = [ordered]@{
    version   = $Version
    notes     = $Notes
    pub_date  = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
    platforms = [ordered]@{
        'windows-x86_64' = [ordered]@{
            signature = (Get-Content $Sig -Raw).Trim()
            url       = $Url
        }
    }
}
$manifest | ConvertTo-Json -Depth 5 | Set-Content $Out -Encoding utf8NoBOM
Get-Content $Out
