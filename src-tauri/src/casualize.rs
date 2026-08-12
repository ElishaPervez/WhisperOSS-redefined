//! Local casual-mode formatting (no AI call). Whisper already returns
//! punctuated, capitalized text; casual style is a deterministic rewrite:
//! all lowercase, no sentence-ending periods, no commas. Question marks,
//! exclamation points, apostrophes, decimals, ellipses, and dotted names
//! like example.com survive untouched.

pub fn casualize(text: &str) -> String {
    let chars: Vec<char> = text.to_lowercase().chars().collect();
    let mut out = String::with_capacity(chars.len());
    for (i, &c) in chars.iter().enumerate() {
        let prev = i.checked_sub(1).map(|p| chars[p]);
        let next = chars.get(i + 1).copied();
        let keep = match c {
            // A lone dot before whitespace/end is sentence punctuation and
            // goes. Dots in a run (ellipsis) or glued to what follows
            // (3.5, example.com) stay.
            '.' => {
                let in_run = prev == Some('.') || next == Some('.');
                let glued = next.is_some_and(|n| !n.is_whitespace());
                in_run || glued
            }
            // Commas only survive as digit grouping (1,000).
            ',' => {
                prev.is_some_and(|p| p.is_ascii_digit())
                    && next.is_some_and(|n| n.is_ascii_digit())
            }
            _ => true,
        };
        if keep {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::casualize;

    #[test]
    fn lowercases_and_drops_commas() {
        assert_eq!(
            casualize("Bro, I'm not getting anything"),
            "bro i'm not getting anything"
        );
    }

    #[test]
    fn strips_sentence_ending_periods() {
        assert_eq!(casualize("I did. Then I left."), "i did then i left");
    }

    #[test]
    fn keeps_question_marks() {
        assert_eq!(casualize("Are you coming?"), "are you coming?");
    }

    #[test]
    fn keeps_exclamation_points() {
        assert_eq!(casualize("No way!"), "no way!");
    }

    #[test]
    fn keeps_decimals_and_dotted_names() {
        assert_eq!(
            casualize("Version 3.5 is on example.com."),
            "version 3.5 is on example.com"
        );
    }

    #[test]
    fn keeps_ellipses() {
        assert_eq!(casualize("Wait... what?"), "wait... what?");
    }

    #[test]
    fn keeps_commas_inside_numbers() {
        assert_eq!(casualize("1,000 people, wow."), "1,000 people wow");
    }

    #[test]
    fn empty_stays_empty() {
        assert_eq!(casualize(""), "");
    }
}
