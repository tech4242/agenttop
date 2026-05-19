# homebrew/agenttop.rb — source of truth for the Homebrew formula.
#
# The release workflow renders this template (substituting VERSION and the
# four SHA256s) and pushes the result to tech4242/homebrew-agenttop, so
# `brew install tech4242/agenttop/agenttop` installs the latest release.
#
# Placeholders below (REPLACE_*) are filled in by the release workflow.
# Hand-edits to this file should preserve the placeholder strings.
class Agenttop < Formula
  desc "htop for AI coding agents — terminal observability for Claude, Codex, Gemini, and more"
  homepage "https://github.com/tech4242/agenttop"
  version "REPLACE_VERSION"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/tech4242/agenttop/releases/download/v#{version}/agenttop-darwin-arm64.tar.gz"
      sha256 "REPLACE_SHA256_DARWIN_ARM64"
    end
    on_intel do
      url "https://github.com/tech4242/agenttop/releases/download/v#{version}/agenttop-darwin-x86_64.tar.gz"
      sha256 "REPLACE_SHA256_DARWIN_X86_64"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/tech4242/agenttop/releases/download/v#{version}/agenttop-linux-aarch64.tar.gz"
      sha256 "REPLACE_SHA256_LINUX_AARCH64"
    end
    on_intel do
      url "https://github.com/tech4242/agenttop/releases/download/v#{version}/agenttop-linux-x86_64.tar.gz"
      sha256 "REPLACE_SHA256_LINUX_X86_64"
    end
  end

  def install
    bin.install "agenttop"
  end

  test do
    assert_match "agenttop #{version}", shell_output("#{bin}/agenttop --version")
  end
end
