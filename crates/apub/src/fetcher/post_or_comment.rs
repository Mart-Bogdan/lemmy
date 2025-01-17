use crate::objects::{comment::Note, post::Page, FromApub};
use activitystreams::chrono::NaiveDateTime;
use diesel::PgConnection;
use lemmy_apub_lib::traits::ApubObject;
use lemmy_db_schema::source::{
  comment::{Comment, CommentForm},
  post::{Post, PostForm},
};
use lemmy_utils::LemmyError;
use lemmy_websocket::LemmyContext;
use serde::Deserialize;
use url::Url;

#[derive(Clone, Debug)]
pub enum PostOrComment {
  Post(Box<Post>),
  Comment(Comment),
}

pub enum PostOrCommentForm {
  PostForm(Box<PostForm>),
  CommentForm(CommentForm),
}

#[derive(Deserialize)]
pub enum PageOrNote {
  Page(Box<Page>),
  Note(Box<Note>),
}

#[async_trait::async_trait(?Send)]
impl ApubObject for PostOrComment {
  type DataType = PgConnection;

  fn last_refreshed_at(&self) -> Option<NaiveDateTime> {
    None
  }

  // TODO: this can probably be implemented using a single sql query
  fn read_from_apub_id(conn: &PgConnection, object_id: Url) -> Result<Option<Self>, LemmyError>
  where
    Self: Sized,
  {
    let post = Post::read_from_apub_id(conn, object_id.clone())?;
    Ok(match post {
      Some(o) => Some(PostOrComment::Post(Box::new(o))),
      None => Comment::read_from_apub_id(conn, object_id)?.map(PostOrComment::Comment),
    })
  }
}

#[async_trait::async_trait(?Send)]
impl FromApub for PostOrComment {
  type ApubType = PageOrNote;

  async fn from_apub(
    apub: &PageOrNote,
    context: &LemmyContext,
    expected_domain: &Url,
    request_counter: &mut i32,
  ) -> Result<Self, LemmyError>
  where
    Self: Sized,
  {
    Ok(match apub {
      PageOrNote::Page(p) => PostOrComment::Post(Box::new(
        Post::from_apub(p, context, expected_domain, request_counter).await?,
      )),
      PageOrNote::Note(n) => PostOrComment::Comment(
        Comment::from_apub(n, context, expected_domain, request_counter).await?,
      ),
    })
  }
}

impl PostOrComment {
  pub(crate) fn ap_id(&self) -> Url {
    match self {
      PostOrComment::Post(p) => p.ap_id.clone(),
      PostOrComment::Comment(c) => c.ap_id.clone(),
    }
    .into()
  }
}
