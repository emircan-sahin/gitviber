# Template for Casks/gitviber.rb in emircan-sahin/homebrew-tap. The release workflow fills in
# the version and the .dmg's checksum, drops these comment lines and prints the result.
cask "gitviber" do
  version "__VERSION__"
  sha256 "__SHA256__"

  url "https://github.com/emircan-sahin/gitviber/releases/download/v#{version}/GitViber_#{version}_universal.dmg"
  name "GitViber"
  desc "Git client for reviewing and committing what coding agents write"
  homepage "https://github.com/emircan-sahin/gitviber"

  livecheck do
    url :url
    strategy :github_latest
  end

  auto_updates true
  depends_on macos: ">= :ventura"

  app "GitViber.app"

  zap trash: [
    "~/Library/Caches/app.gitviber.desktop",
    "~/Library/Logs/app.gitviber.desktop",
    "~/Library/Preferences/app.gitviber.desktop.plist",
    "~/Library/WebKit/app.gitviber.desktop",
  ]
end
