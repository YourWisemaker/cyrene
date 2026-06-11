# Homebrew formula for Cyrene.
#
# Intended for a tap (e.g. `brew tap YourWisemaker/cyrene && brew install cyrene`).
# Copy this file to a `homebrew-cyrene` tap repo as `Formula/cyrene.rb`, or the
# release workflow can sync it automatically. It installs the prebuilt binary
# for the host platform — no Rust toolchain required.
#
# After publishing a release, update `version` and the four `sha256` values with
# the checksums from the `*.sha256` files attached to the GitHub Release.
class Cyrene < Formula
  desc "The AI agent that always loves you"
  homepage "https://github.com/YourWisemaker/cyrene"
  version "0.1.0"
  license "Apache-2.0"

  on_macos do
    on_arm do
      url "https://github.com/YourWisemaker/cyrene/releases/download/v#{version}/cyrene-aarch64-apple-darwin.tar.gz"
      sha256 "REPLACE_WITH_aarch64-apple-darwin_SHA256"
    end
    on_intel do
      url "https://github.com/YourWisemaker/cyrene/releases/download/v#{version}/cyrene-x86_64-apple-darwin.tar.gz"
      sha256 "REPLACE_WITH_x86_64-apple-darwin_SHA256"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/YourWisemaker/cyrene/releases/download/v#{version}/cyrene-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "REPLACE_WITH_aarch64-unknown-linux-gnu_SHA256"
    end
    on_intel do
      url "https://github.com/YourWisemaker/cyrene/releases/download/v#{version}/cyrene-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "REPLACE_WITH_x86_64-unknown-linux-gnu_SHA256"
    end
  end

  def install
    # The release tarball unpacks into a `cyrene-<target>/` directory.
    bin.install Dir["**/cyrene"].first => "cyrene"
  end

  test do
    assert_match "Cyrene", shell_output("#{bin}/cyrene --version")
  end
end
