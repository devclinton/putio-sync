class PutioSync < Formula
  desc "CLI tool for syncing files from Put.io to local storage"
  homepage "https://github.com/clintoncollins/putio-sync"
  url "https://github.com/clintoncollins/putio-sync/archive/refs/tags/v0.1.1.tar.gz"
  sha256 "CHECKSUM_HERE"  # This will be updated automatically by release workflow
  license "MIT"
  version "0.1.1"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args(path: "putio-sync-cli")
  end

  test do
    assert_match "putio-sync", shell_output("#{bin}/putio-sync --version")
  end

  # Local testing instructions:
  # 1. Build from source:
  #    brew install --build-from-source ./packaging/homebrew/putio-sync.rb
  # 2. Or install locally without checksum verification:
  #    brew install --formula --build-from-source --ignore-dependencies ./packaging/homebrew/putio-sync.rb
  # 3. Uninstall:
  #    brew uninstall putio-sync
  #
  # Note: For local testing, you may need to modify the url to point to a local tarball
  # or use a git-based installation temporarily.
end
