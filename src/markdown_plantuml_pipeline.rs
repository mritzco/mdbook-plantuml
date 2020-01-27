use std::string::String;

pub trait PlantUMLCodeBlockRenderer {
    ///Renders the given code block and returns a markdown link to the generated
    ///image. E.g. ![image](/img/foobar.svg).
    // On failure the returned string is simply the error description.
    ///Note that the prepocessor can never output HTML! mdBook will not render
    ///anything after HTML code for some reason. So markdown in, markdown out.
    //TODO: code block can be a reference
    fn render(&self, code_block: String, rel_image_url: &String) -> String;
}

pub fn render_plantuml_code_blocks(
    markdown: &str,
    renderer: &impl PlantUMLCodeBlockRenderer,
    rel_image_url: &String,
) -> String {
    let processor = PlantUMLCodeProcessor::new(markdown);
    processor.process(renderer, rel_image_url)
}

/// Find the first byte not equal to the expected byte
/// Only works for ASCII bytes (in this context ' ', '~' and '`' ), which should
/// work fine for unicode stuff too.
/// # Arguments
/// * `bytes` - The bytes array to parse
/// * `expected` - The char to compare against (as a byte value)
/// * `start` - The start offset for the search
fn find_first_inequal(bytes: &[u8], expected: u8, start: usize) -> usize {
    let mut pos = start;

    while pos < bytes.len() && bytes[pos] == expected {
        pos += 1;
    }

    pos
}

/// Find the next line in the given byte array
/// # Arguments
/// * `bytes` - The bytes array to parse
/// * `start` - The start offset for the search
fn next_line(bytes: &[u8], start: usize) -> usize {
    let mut pos = start;
    while pos < bytes.len() && bytes[pos] != b'\n' {
        pos += 1;
    }

    pos + 1
}

/// Find the next code fence (start, or end fence) in the given byte array
/// # Arguments
/// * `bytes` - The bytes array to parse
/// * `start` - The start offset for the search
/// * `min_length` - Optional length of the code fence to find (used for finding the closing fence)
/// * `fence_char` - Optional fence char to match (used for finding the closing fence)
fn find_next_code_fence(
    bytes: &[u8],
    start: usize,
    min_length: Option<usize>,
    fence_char: Option<u8>,
) -> Option<(usize, usize)> {
    if bytes.len() < 3 {
        return None;
    }

    let mut pos: usize = start;

    let is_fence_char = |c| {
        // TODO: there is probably a more optimal way of doing this
        if let Some(expected) = fence_char {
            expected == c
        } else {
            (c == b'`' || c == b'~')
        }
    };

    let is_fence = |s, e| {
        if let Some(l) = min_length {
            (e - s) >= l
        } else {
            (e - s) >= 3
        }
    };

    while pos < bytes.len() {
        let line_start = pos;
        pos = find_first_inequal(bytes, b' ', pos);

        if (pos - line_start) < 4 && is_fence_char(bytes[pos]) {
            let first_non_fence = find_first_inequal(bytes, bytes[pos], pos);
            if is_fence(pos, first_non_fence) {
                return Some((pos, first_non_fence));
            }

            pos = first_non_fence;
        }

        pos = next_line(bytes, pos);
    }

    None
}

/// Gets the code block's info string, or None if it cannot be found.
/// # Arguments
/// * `bytes` - The bytes array to parse
/// * `fence_end` - The start offset for the search
/// * `min_length` - Optional length of the code fence to find (used for finding the closing)
fn get_info_string(bytes: &[u8], fence_end: usize) -> Option<&str> {
    let info_start = find_first_inequal(bytes, b' ', fence_end);
    if info_start < bytes.len() {
        let mut pos = info_start;
        while pos < bytes.len() && bytes[pos] != b'\n' && bytes[pos] != b' ' && bytes[pos] != b'\r'
        {
            pos += 1;
        }

        if pos > info_start {
            if let Ok(info) = std::str::from_utf8(&bytes[info_start..pos]) {
                return Some(info);
            }
        }
    }

    None
}

struct CodeBlock<'a> {
    /// The code block's code slice (stripped from fences and info string)
    code: &'a str,
    /// The code block's info string (if any)
    info_string: Option<&'a str>,
    /// Byte offset of first character of opening fence
    start_pos: usize,
    /// Byte offset of newline after closing fence
    end_pos: usize,
}

impl<'a> CodeBlock<'a> {
    fn is_plantuml(&self) -> bool {
        self.info_string.unwrap_or(&"") == "plantuml"
    }
}

struct PlantUMLCodeProcessor<'a> {
    markdown: &'a str,
}

impl<'a> PlantUMLCodeProcessor<'a> {
    pub fn new(markdown: &str) -> PlantUMLCodeProcessor {
        PlantUMLCodeProcessor { markdown: markdown }
    }

    /// Returns the byte offsets of the (optional) end fence and code end
    /// positions as a tuple.
    /// Returns bytes.len() for both if the end fence is None
    /// # Arguments
    /// * `bytes` - The bytes array to parse
    /// * `fence_end` - Option with the byte offsets of the end fence
    fn get_end_positions(&self, bytes: &[u8], fence_end: Option<(usize, usize)>) -> (usize, usize) {
        let code_end;
        let end_pos;

        if let Some((ce, fe)) = fence_end {
            code_end = ce;
            end_pos = {
                let p = next_line(bytes, fe);
                if p != bytes.len() {
                    p - 1
                } else {
                    p
                }
            }
        } else {
            code_end = bytes.len();
            end_pos = code_end;
        }

        (code_end, end_pos)
    }

    /// Get next code block in document, starting at byte offset start_pos
    /// Returns None if no more code blocks are found.
    fn get_next_code_block(&self, start_pos: usize) -> Option<CodeBlock> {
        let bytes = self.markdown.as_bytes();
        if let Some((s, e)) = find_next_code_fence(bytes, start_pos, None, None) {
            let info_string = get_info_string(bytes, e);
            let code_start = next_line(bytes, e);
            let fence_end = find_next_code_fence(bytes, e, Some(e - s), Some(bytes[s]));
            let (code_end, end_pos) = self.get_end_positions(bytes, fence_end);

            return Some(CodeBlock {
                code: &self.markdown[code_start..code_end],
                info_string: info_string,
                start_pos: s,
                end_pos: end_pos,
            });
        }

        None
    }

    /// Processes all code blocks in the document (self.markdown)
    /// Replaces every "plantuml" code block with the renderer output.
    /// Returns the processed markdown.
    /// # Arguments
    /// * `renderer` - The renderer to use for the "plantuml" code blocks
    /// * `rel_image_url` - The url of the image relative to the book output dir.
    pub fn process(
        &self,
        renderer: &impl PlantUMLCodeBlockRenderer,
        rel_image_url: &String,
    ) -> String {
        let mut processed = String::new();
        processed.reserve(self.markdown.len());

        let bytes = self.markdown.as_bytes();
        let mut start_pos: usize = 0;
        while start_pos < bytes.len() {
            if let Some(code_block) = self.get_next_code_block(start_pos) {
                if code_block.is_plantuml() {
                    processed.push_str(&self.markdown[start_pos..code_block.start_pos]);
                    let rendered = renderer.render(String::from(code_block.code), rel_image_url);
                    processed.push_str(rendered.as_str());
                } else {
                    processed.push_str(&self.markdown[start_pos..code_block.end_pos]);
                }
                start_pos = code_block.end_pos;
            } else {
                processed.push_str(&self.markdown[start_pos..]);
                start_pos = bytes.len();
            }
        }

        processed
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::cell::RefCell;

    struct FakeRenderer {
        ///TODO: Make this a vector
        code_block: RefCell<String>,
    }

    impl PlantUMLCodeBlockRenderer for FakeRenderer {
        fn render(&self, code_block: String, _rel_image_url: &String) -> String {
            self.code_block.replace(code_block);
            String::from("rendered")
        }
    }

    #[test]
    fn test_find_next_code_fence() {
        macro_rules! assert_find_next_code_fence {
            ($expected_slice_opt:expr, $markdown:expr, $start:expr, $min_length: expr, $fence_char: expr) => {{
                let bytes = $markdown.as_bytes();
                let fence_range = find_next_code_fence(bytes, $start, $min_length, $fence_char);
                if let Some((s, e)) = $expected_slice_opt {
                    assert!(fence_range.is_some());
                    assert_eq!((s, e), fence_range.unwrap());
                } else {
                    assert!(fence_range.is_none());
                }
            }};
        };

        assert_find_next_code_fence!(None, "", 0, None, None);
        assert_find_next_code_fence!(None, "a\n\n", 0, None, None);
        assert_find_next_code_fence!(None, "a```", 0, None, None);

        //Only spaces before the fence chars, _nothing_ else
        assert_find_next_code_fence!(None, "\\ ```", 0, None, None);

        // At least 3 chars
        assert_find_next_code_fence!(None, "``", 0, None, None);
        assert_find_next_code_fence!(Some((0, 3)), "```", 0, None, None);
        assert_find_next_code_fence!(Some((0, 4)), "````", 0, None, None);
        assert_find_next_code_fence!(Some((0, 5)), "`````", 0, None, None);
        assert_find_next_code_fence!(None, "~~", 0, None, None);
        assert_find_next_code_fence!(Some((0, 3)), "~~~", 0, None, None);
        assert_find_next_code_fence!(Some((0, 4)), "~~~~", 0, None, None);
        assert_find_next_code_fence!(Some((0, 5)), "~~~~~", 0, None, None);

        // At most 3 spaces indent
        assert_find_next_code_fence!(Some((1, 4)), " ```", 0, None, None);
        assert_find_next_code_fence!(Some((2, 5)), "  ```", 0, None, None);
        assert_find_next_code_fence!(Some((3, 6)), "   ```", 0, None, None);
        assert_find_next_code_fence!(None, "    ```", 0, None, None);

        //Somewhere further in the document
        assert_find_next_code_fence!(Some((4, 7)), "abc\n~~~\n", 0, None, None);
        assert_find_next_code_fence!(Some((10, 14)), "abc\n~~\n\n  ````\n", 0, None, None);

        //Somewhere further in the document with windows line endings
        assert_find_next_code_fence!(Some((5, 8)), "abc\r\n~~~\r\n", 0, None, None);
        assert_find_next_code_fence!(Some((13, 17)), "abc\r\n~~\r\n\r\n  ````\r\n", 0, None, None);

        //Find specific min length
        assert_find_next_code_fence!(Some((4, 8)), "```\n````", 0, Some(4), None);
        assert_find_next_code_fence!(Some((4, 10)), "```\n``````", 0, Some(4), None);

        //Start offset
        assert_find_next_code_fence!(Some((5, 8)), "```  ```", 3, None, None);
        assert_find_next_code_fence!(Some((8, 11)), "```\n~~~\n```", 3, Some(3), Some(b'`'));

        //Rest
        assert_find_next_code_fence!(Some((0, 3)), "``` ```", 0, None, None);
        assert_find_next_code_fence!(None, "``~~~", 0, None, None);
    }

    #[test]
    fn test_get_info_string() {
        macro_rules! assert_get_info_string {
            ($markdown:expr, $start:expr, $expected_range: expr) => {{
                let bytes = $markdown.as_bytes();

                let slice = get_info_string(bytes, $start);
                if let Some((s, e)) = $expected_range {
                    assert!(slice.is_some());
                    assert_eq!(&$markdown[s..e], slice.unwrap());
                } else {
                    assert!(slice.is_none());
                }
            }};
        };

        assert_get_info_string!("", 0, None);
        assert_get_info_string!("  ", 0, None);
        assert_get_info_string!("\n", 0, None);

        assert_get_info_string!("foobar", 0, Some((0, 6)));
        assert_get_info_string!("foobar\n", 0, Some((0, 6)));
        assert_get_info_string!("foobar\r\n", 0, Some((0, 6)));
        assert_get_info_string!("foobar ", 0, Some((0, 6)));

        assert_get_info_string!("foobar baz", 0, Some((0, 6)));
        assert_get_info_string!("  foobar  \n", 0, Some((2, 8)));
        assert_get_info_string!("  foobar baz \n", 0, Some((2, 8)));

        assert_get_info_string!("some```foobar", 7, Some((7, 13)));
    }

    #[test]
    fn test_process_plantuml_code() {
        macro_rules! assert_plantuml_injection {
            ($markdown:expr, $expected_code_block:expr, $rendered_output:expr) => {{
                let processor = PlantUMLCodeProcessor::new($markdown);
                let renderer = FakeRenderer {
                    code_block: RefCell::new(String::new()),
                };
                let result = processor.process(&renderer, &String::from(""));
                assert_eq!($expected_code_block, *renderer.code_block.borrow());
                assert_eq!($rendered_output, result);
            }};
        }

        assert_plantuml_injection!("```plantuml\nfoo\n```", "foo\n", "rendered");
        assert_plantuml_injection!(
            "abc\n```plantuml\nfoo\n```\ndef",
            "foo\n",
            "abc\nrendered\ndef"
        );
        assert_plantuml_injection!("abc\n```plantuml\nfoo", "foo", "abc\nrendered");
        assert_plantuml_injection!(
            "abc\n```plantuml\nfoo\n```\ndef\n```plantuml\nbar\n```\ngeh",
            "bar\n",
            "abc\nrendered\ndef\nrendered\ngeh"
        );
        assert_plantuml_injection!(
            "abc\n```plantuml\nfoo\n```\ndef\n```plantuml\nbar",
            "bar",
            "abc\nrendered\ndef\nrendered"
        );
        assert_plantuml_injection!(
            "abc\n```\nfoo\n```\ndef\n```plantuml\nbar",
            "bar",
            "abc\n```\nfoo\n```\ndef\nrendered"
        );
    }
}
