use nero_processor::Processor;

pub trait AsyncTryFromWithProcessor<T>: Sized {
    async fn async_try_from_with_processor(value: T, processor: &Processor)
    -> anyhow::Result<Self>;
}

pub trait AyncTryIntoWithProcessor<T>: Sized {
    async fn async_try_into_with_processor(self, processor: &Processor) -> anyhow::Result<T>;
}

impl<T, U> AyncTryIntoWithProcessor<U> for T
where
    U: AsyncTryFromWithProcessor<T>,
{
    async fn async_try_into_with_processor(self, processor: &Processor) -> anyhow::Result<U> {
        U::async_try_from_with_processor(self, processor).await
    }
}
