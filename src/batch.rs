pub struct Batch<I: Iterator> {
    inner: I,
    sep: I::Item,
    arg_max: usize,
    _first: Option<String>,
}

impl<I> Batch<I>
where
    I: Iterator,
    I::Item: AsRef<str>,
{
    pub fn new(inner: I, arg_max: usize, sep: I::Item) -> Self {
        Self {
            inner,
            sep,
            arg_max,
            _first: None,
        }
    }
}

impl<I> Iterator for Batch<I>
where
    I: Iterator,
    I::Item: AsRef<str> + Default,
{
    type Item = String;

    fn next(&mut self) -> Option<Self::Item> {
        let first = self
            ._first
            .take()
            .unwrap_or_else(|| self.inner.next().unwrap_or_default().as_ref().to_owned());

        if first.len() > self.arg_max {
            return Some(first);
        }

        let mut acc = first;
        for item in self.inner.by_ref() {
            let item = item.as_ref().to_owned();
            if acc.len() + item.len() >= self.arg_max {
                self._first = Some(item);
                return Some(acc);
            }
            acc = acc + self.sep.as_ref() + &item;
        }

        if !acc.is_empty() {
            Some(acc)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! batch_tests {
    ($($name:ident: $value:expr,)*) => {
    $(
        #[test]
        fn $name() {
            let (input, arg_max, expected) = $value;
            let it = input.into_iter();
            let batch = Batch::new(it, arg_max, &";");
            let res = batch.into_iter().collect::<Vec<_>>();
            assert_eq!(expected, res);
        }
    )*
    }
}

    #[test]
    fn test_default() {
        let it = vec!["a"].into_iter();
        let batch = Batch::new(it, 8000, ";");
        assert!(batch.arg_max > 0);
    }

    batch_tests! {
        trival: (Vec::<&str>::new(), 1000, Vec::<&str>::new()),
        min_batch: (vec!["foo", "bar"], 1000, vec!["foo;bar"]),
        everything_over: (vec!["foobar", "barbar"], 2, vec!["foobar", "barbar"]),
        max_arg_arg0: (vec!["foobar", "barbar"], 6, vec!["foobar", "barbar"]),
        max_arg_arg1: (vec!["foobar", "barbar"], 7, vec!["foobar", "barbar"]),
        batch0: (vec!["foo", "bar"], 7, vec!["foo;bar"]),
        batch1: (vec!["foo", "bar", "baz"], 7, vec!["foo;bar", "baz"]),
        batch2: (vec!["foo", "bar", "baz", "frobnicate"], 7, vec!["foo;bar", "baz", "frobnicate"]),
        small_batch0: (
            vec!["a", "b", "c", "d", "e", "f", "g", "h", "i"],
            3,
            vec!["a;b", "c;d", "e;f", "g;h", "i"]
        ),
    }
}
