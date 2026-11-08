    /// A listed object with no recorded size counts as zero bytes rather than failing the
    /// listing: `Size` is optional in the response model, and one object without it must not take
    /// the whole footprint down.
    #[test]
    fn test_listed_size_is_zero_when_the_listing_omits_it() {
        let unsized_object = aws_sdk_s3::types::Object::builder()
            .key("staging/c/0.png")
            .build();
        assert_eq!(listed_size(&unsized_object), 0);

        let sized_object = aws_sdk_s3::types::Object::builder()
            .key("staging/c/0.png")
            .size(4096)
            .build();
        assert_eq!(listed_size(&sized_object), 4096);
    }
}
