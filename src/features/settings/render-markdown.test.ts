import { describe, it, expect } from "vitest";
import { renderMarkdown } from "./render-markdown";

describe("renderMarkdown", () => {
  it("renders bold, italic and inline code", () => {
    expect(renderMarkdown("**bold**")).toBe("<p><strong>bold</strong></p>");
    expect(renderMarkdown("*em*")).toBe("<p><em>em</em></p>");
    expect(renderMarkdown("_em_")).toBe("<p><em>em</em></p>");
    expect(renderMarkdown("use `code` here")).toBe(
      "<p>use <code>code</code> here</p>",
    );
  });

  it("does not collide placeholders with ' digit ' text", () => {
    // A bare ' 0 ' in text must survive (regression guard for the sentinel).
    expect(renderMarkdown("bump version 0 today")).toBe(
      "<p>bump version 0 today</p>",
    );
    expect(renderMarkdown("`x` and 0 and 1")).toBe(
      "<p><code>x</code> and 0 and 1</p>",
    );
  });

  it("renders headings", () => {
    expect(renderMarkdown("# Title")).toBe(
      '<div class="md-h md-h1">Title</div>',
    );
    expect(renderMarkdown("### Sub")).toBe('<div class="md-h md-h3">Sub</div>');
  });

  it("renders unordered and ordered lists", () => {
    expect(renderMarkdown("- a\n- b")).toBe("<ul><li>a</li><li>b</li></ul>");
    expect(renderMarkdown("1. a\n2. b")).toBe("<ol><li>a</li><li>b</li></ol>");
  });

  it("renders fenced code blocks without inline processing", () => {
    expect(renderMarkdown("```\n**not bold**\n```")).toBe(
      "<pre><code>**not bold**</code></pre>",
    );
  });

  it("renders a horizontal rule", () => {
    expect(renderMarkdown("---")).toBe("<hr>");
  });

  it("allows http/https/mailto links only", () => {
    expect(renderMarkdown("[site](https://example.com)")).toBe(
      '<p><a href="https://example.com" target="_blank" rel="noopener noreferrer">site</a></p>',
    );
    // Unsafe scheme is left as literal text, not turned into a link.
    expect(renderMarkdown("[x](javascript:alert(1))")).toBe(
      "<p>[x](javascript:alert(1))</p>",
    );
  });

  it("escapes HTML to prevent injection", () => {
    expect(renderMarkdown("<script>alert(1)</script>")).toBe(
      "<p>&lt;script&gt;alert(1)&lt;/script&gt;</p>",
    );
    expect(renderMarkdown('a & b "c"')).toBe("<p>a &amp; b &quot;c&quot;</p>");
  });

  it("merges plain lines into a paragraph with soft breaks", () => {
    expect(renderMarkdown("line one\nline two")).toBe(
      "<p>line one<br>line two</p>",
    );
  });

  it("separates blocks split by blank lines", () => {
    expect(renderMarkdown("# Features\n- a\n\npara")).toBe(
      '<div class="md-h md-h1">Features</div><ul><li>a</li></ul><p>para</p>',
    );
  });
});
