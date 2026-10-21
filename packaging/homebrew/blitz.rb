cask "blitz" do
  version "0.1.1"
  sha256 "2ba649458e49d2b4948a619ed409a13d0bb7097bc960cc1ec5876497d25241c1"

  url "https://github.com/h3ma209/Blitz/releases/download/v#{version}/Blitz-macos-arm64.dmg"
  name "Blitz"
  desc "Lightweight, native, offline-first API client in Rust"
  homepage "https://h3ma209.github.io/Blitz/"

  depends_on arch: :arm64

  app "Blitz.app"
  binary "#{appdir}/Blitz.app/Contents/MacOS/Blitz", target: "blitz"

  zap trash: [
    "~/Library/Application Support/blitz",
    "~/Library/Caches/blitz",
  ]
end
