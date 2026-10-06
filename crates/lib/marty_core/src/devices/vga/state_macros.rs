// Getters/setters handle explicit exceptional encodings; ordinary fields use copy.
macro_rules! vga_get {
    ($c:expr, $field:ident, copy) => { $c.$field.clone() };
    ($c:expr, $field:ident, ($get:expr, $put:expr)) => { ($get)($c) };
}
macro_rules! vga_put {
    ($s:expr, $field:ident, copy) => { $s.$field.clone() };
    ($s:expr, $field:ident, ($get:expr, $put:expr)) => { ($put)($s)? };
}
// Lossless VGA owner codecs. Field inventories construct the complete native
// owner explicitly, so newly added owner fields require a codec update.
macro_rules! vga_state {
    ($owner:ident, $state:ident, {$($skip:ident: $default:expr,)*},
     {$($(#[$attr:meta])* $field:ident: $ty:ty =>  $codec:tt,)*}) => {
        #[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        pub(crate) struct $state {
            $($(#[$attr])* pub(crate) $field: $ty,)*
        }
        impl $owner {
            pub(crate) fn snapshot_state(&self) -> $state {
                $state { $($field: vga_get!(self, $field, $codec),)* }
            }
            pub(crate) fn prepare_state(s: &$state) -> Result<Self, &'static str> {
                Self::validate_state(s)?;
                Ok(Self { $($field: vga_put!(s, $field, $codec),)* $($skip: $default,)* })
            }
        }
    }
}
