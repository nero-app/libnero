use nero_media_proxy::MediaProxy;

pub trait AsyncTryFromWithProxy<T>: Sized {
    async fn async_try_from_with_proxy(value: T, proxy: &MediaProxy) -> anyhow::Result<Self>;
}

pub trait AyncTryIntoWithProxy<T>: Sized {
    async fn async_try_into_with_proxy(self, proxy: &MediaProxy) -> anyhow::Result<T>;
}

impl<T, U> AyncTryIntoWithProxy<U> for T
where
    U: AsyncTryFromWithProxy<T>,
{
    async fn async_try_into_with_proxy(self, proxy: &MediaProxy) -> anyhow::Result<U> {
        U::async_try_from_with_proxy(self, proxy).await
    }
}
