# 1) Let release-plz bump, commit and tag
release-plz update --allow-dirty

# 2) Grab the 5-char SHA of that new HEAD
$sha = git rev-parse --short=7 HEAD

# 3) Pull the version out of Cargo.toml
$version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"' |
            ForEach-Object { $_.Matches[0].Groups[1].Value })

# 4) Format today’s date as yy/MM/dd
$date = Get-Date -Format 'yy/MM/dd'

# 7) Build the LAST_RELEASE content
$content = "$date|$sha|$version"

# 6) Write LAST_RELEASE (no trailing newline)
Set-Content -Path LAST_RELEASE -Value $content -Encoding UTF8

# 7) Amend the release-plz commit to include LAST_RELEASE
git add LAST_RELEASE
git commit --amend --no-edit
