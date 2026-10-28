# Homebrew formula for matcha, yuzu, ube, and taro. This repo is its own tap:
#   brew tap trammel-may/mochi-donut https://github.com/trammel-may/mochi-donut
#   brew install --HEAD mochi-donut
# HEAD-only until the first release (docs/decisions/0011-homebrew-tap.md).
class MochiDonut < Formula
  desc "Tools that help AI coding agents work with evidence and in the right environment"
  homepage "https://github.com/trammel-may/mochi-donut"
  license :cannot_represent
  head "https://github.com/trammel-may/mochi-donut.git", branch: "main"

  depends_on "rust" => :build

  conflicts_with "matcha", because: "both install a `matcha` binary"

  def install
    system "cargo", "install", *std_cargo_args(path: "crates/matcha")
    system "cargo", "install", *std_cargo_args(path: "crates/yuzu")
    system "cargo", "install", *std_cargo_args(path: "crates/ube")
    system "cargo", "install", *std_cargo_args(path: "crates/taro")
  end

  test do
    assert_match "matcha #{version}", shell_output("#{bin}/matcha --version") if build.stable?
    assert_match "When to run what", shell_output("#{bin}/matcha agent-guide")
    assert_match "When to run what", shell_output("#{bin}/yuzu agent-guide")
    assert_match "When to run what", shell_output("#{bin}/ube agent-guide")
    assert_match "When to run what", shell_output("#{bin}/taro agent-guide")

    system "git", "init", "--quiet", testpath/"repo"
    (testpath/"repo/a.rs").write "cancelled\n"
    system "git", "-C", testpath/"repo", "add", "a.rs"
    output = shell_output("#{bin}/matcha --repo #{testpath}/repo census --spelling cancel")
    assert_match "cancelled", output
  end
end
