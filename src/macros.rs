#[macro_export]
macro_rules! c_like_enum {
    ( $name: ident { $($variant: ident = $value: expr,)* } ) => {
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub enum $name {
            $($variant = $value,)+
        }

        impl $name {
            pub fn from_u8(value: u8) -> Option<$name> {
                match value {
                    $($value => Some($name::$variant),)+
                    _ => None
                }
            }
        }
    };
}
