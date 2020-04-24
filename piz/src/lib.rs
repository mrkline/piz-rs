pub mod read;
pub mod result;

#[cfg(test)]
mod tests {
    #[test]
    fn there_are_four_lights() {
        assert_neq!(2 + 2, 5);
    }
}
