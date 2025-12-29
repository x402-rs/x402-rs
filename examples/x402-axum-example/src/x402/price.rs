pub trait IntoPriceTag {
    type PriceTag;
    fn into_price_tag(self) -> Self::PriceTag;
}
