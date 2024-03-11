use std::{collections::VecDeque, task::Poll};

use futures::{
    future::{BoxFuture, OptionFuture},
    Future,
};

use super::WasmerClient;

type PaginationFuture<I, P> = BoxFuture<'static, Result<(Vec<I>, Option<P>), anyhow::Error>>;

pub trait PaginatedQuery {
    type Vars;
    type Paginator;
    type Item;

    fn query(
        &self,
        client: WasmerClient,
        paginator: Option<Self::Paginator>,
    ) -> PaginationFuture<Self::Item, Self::Paginator>;
}

pin_project_lite::pin_project! {
    pub struct QueryStream<Q: PaginatedQuery> {
        query: Q,

        client: WasmerClient,
        page: usize,
        paginator: Option<Q::Paginator>,
        finished: bool,
        items: VecDeque<Q::Item>,

        #[pin]
        fut: OptionFuture<PaginationFuture<Q::Item, Q::Paginator>>,
    }
}

impl<Q: PaginatedQuery> QueryStream<Q> {
    pub fn new(query: Q, client: WasmerClient) -> Self {
        Self {
            query,
            client,
            page: 0,
            finished: false,
            paginator: None,
            items: VecDeque::new(),
            fut: None.into(),
        }
    }
}

impl<Q: PaginatedQuery> futures::Stream for QueryStream<Q> {
    type Item = Result<Q::Item, anyhow::Error>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        if let Some(item) = this.items.pop_front() {
            return Poll::Ready(Some(Ok(item)));
        }

        match this.fut.as_mut().poll(cx) {
            Poll::Ready(None) => {}
            Poll::Ready(Some(Ok((items, paginator)))) => {
                *this.paginator = paginator;
                *this.page += 1;
                // *this.fut = None.into();
                this.items.extend(items);
                this.fut.set(None.into());

                if let Some(item) = this.items.pop_front() {
                    return Poll::Ready(Some(Ok(item)));
                }
            }
            Poll::Ready(Some(Err(err))) => {
                return Poll::Ready(Some(Err(err)));
            }
            Poll::Pending => {
                return Poll::Pending;
            }
        };

        let pager = match this.paginator.take() {
            Some(p) => Some(p),
            None if *this.page == 0 => None,
            None => {
                return Poll::Ready(None);
            }
        };

        let f = this.query.query(this.client.clone(), pager);
        this.fut.set(Some(f).into());

        match this.fut.as_mut().poll(cx) {
            Poll::Ready(None) => {
                unreachable!()
            }
            Poll::Ready(Some(Ok((items, paginator)))) => {
                *this.paginator = paginator;
                *this.page += 1;
                // *this.fut = None.into();
                this.items.extend(items);
                this.fut.set(None.into());

                if let Some(item) = this.items.pop_front() {
                    Poll::Ready(Some(Ok(item)))
                } else {
                    Poll::Ready(None)
                }
            }
            Poll::Ready(Some(Err(err))) => Poll::Ready(Some(Err(err))),
            Poll::Pending => Poll::Pending,
        }
    }
}
