cask "blitz" do
  version "0.1.0"
  sha256 "ac29a1b4e96d0331f0dc7ab699453893433f340bd27aaab75f331bb19b8c9ba3"

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
