use crate::{
  activities::{
    deletion::{delete::Delete, undo_delete::UndoDelete},
    verify_mod_action,
    verify_person_in_community,
  },
  fetcher::object_id::ObjectId,
};
use diesel::PgConnection;
use lemmy_api_common::blocking;
use lemmy_apub_lib::{
  traits::{ActivityFields, ActorType, ApubObject},
  verify::verify_domains_match,
};
use lemmy_db_queries::source::{comment::Comment_, community::Community_, post::Post_};
use lemmy_db_schema::source::{comment::Comment, community::Community, person::Person, post::Post};
use lemmy_utils::LemmyError;
use lemmy_websocket::{
  send::{send_comment_ws_message_simple, send_community_ws_message, send_post_ws_message},
  LemmyContext,
  UserOperationCrud,
};
use url::Url;

pub mod delete;
pub mod undo_delete;

pub async fn send_apub_delete(
  actor: &Person,
  community: &Community,
  object_id: Url,
  deleted: bool,
  context: &LemmyContext,
) -> Result<(), LemmyError> {
  if deleted {
    Delete::send(actor, community, object_id, None, context).await
  } else {
    UndoDelete::send(actor, community, object_id, None, context).await
  }
}

// TODO: remove reason is actually optional in lemmy. we set an empty string in that case, but its
//       ugly
pub async fn send_apub_remove(
  actor: &Person,
  community: &Community,
  object_id: Url,
  reason: String,
  removed: bool,
  context: &LemmyContext,
) -> Result<(), LemmyError> {
  if removed {
    Delete::send(actor, community, object_id, Some(reason), context).await
  } else {
    UndoDelete::send(actor, community, object_id, Some(reason), context).await
  }
}

pub enum DeletableObjects {
  Community(Box<Community>),
  Comment(Box<Comment>),
  Post(Box<Post>),
}

impl DeletableObjects {
  pub(crate) async fn read_from_db(
    ap_id: &Url,
    context: &LemmyContext,
  ) -> Result<DeletableObjects, LemmyError> {
    if let Some(c) =
      DeletableObjects::read_type_from_db::<Community>(ap_id.clone(), context).await?
    {
      return Ok(DeletableObjects::Community(Box::new(c)));
    }
    if let Some(p) = DeletableObjects::read_type_from_db::<Post>(ap_id.clone(), context).await? {
      return Ok(DeletableObjects::Post(Box::new(p)));
    }
    if let Some(c) = DeletableObjects::read_type_from_db::<Comment>(ap_id.clone(), context).await? {
      return Ok(DeletableObjects::Comment(Box::new(c)));
    }
    Err(diesel::NotFound.into())
  }

  // TODO: a method like this should be provided by fetcher module
  async fn read_type_from_db<Type>(
    ap_id: Url,
    context: &LemmyContext,
  ) -> Result<Option<Type>, LemmyError>
  where
    Type: ApubObject<DataType = PgConnection> + Send + 'static,
  {
    blocking(context.pool(), move |conn| {
      Type::read_from_apub_id(conn, ap_id)
    })
    .await?
  }
}

pub(in crate::activities) async fn verify_delete_activity(
  object: &Url,
  activity: &dyn ActivityFields,
  community_id: &ObjectId<Community>,
  is_mod_action: bool,
  context: &LemmyContext,
  request_counter: &mut i32,
) -> Result<(), LemmyError> {
  let object = DeletableObjects::read_from_db(object, context).await?;
  let actor = ObjectId::new(activity.actor().clone());
  match object {
    DeletableObjects::Community(c) => {
      if c.local {
        // can only do this check for local community, in remote case it would try to fetch the
        // deleted community (which fails)
        verify_person_in_community(&actor, community_id, context, request_counter).await?;
      }
      // community deletion is always a mod (or admin) action
      verify_mod_action(
        &actor,
        ObjectId::new(c.actor_id()),
        context,
        request_counter,
      )
      .await?;
    }
    DeletableObjects::Post(p) => {
      verify_delete_activity_post_or_comment(
        activity,
        &p.ap_id.into(),
        community_id,
        is_mod_action,
        context,
        request_counter,
      )
      .await?;
    }
    DeletableObjects::Comment(c) => {
      verify_delete_activity_post_or_comment(
        activity,
        &c.ap_id.into(),
        community_id,
        is_mod_action,
        context,
        request_counter,
      )
      .await?;
    }
  }
  Ok(())
}

async fn verify_delete_activity_post_or_comment(
  activity: &dyn ActivityFields,
  object_id: &Url,
  community_id: &ObjectId<Community>,
  is_mod_action: bool,
  context: &LemmyContext,
  request_counter: &mut i32,
) -> Result<(), LemmyError> {
  let actor = ObjectId::new(activity.actor().clone());
  verify_person_in_community(&actor, community_id, context, request_counter).await?;
  if is_mod_action {
    verify_mod_action(&actor, community_id.clone(), context, request_counter).await?;
  } else {
    // domain of post ap_id and post.creator ap_id are identical, so we just check the former
    verify_domains_match(activity.actor(), object_id)?;
  }
  Ok(())
}

struct WebsocketMessages {
  community: UserOperationCrud,
  post: UserOperationCrud,
  comment: UserOperationCrud,
}

/// Write deletion or restoring of an object to the database, and send websocket message.
/// TODO: we should do something similar for receive_remove_action(), but its much more complicated
///       because of the mod log
async fn receive_delete_action(
  object: &Url,
  actor: &ObjectId<Person>,
  ws_messages: WebsocketMessages,
  deleted: bool,
  context: &LemmyContext,
  request_counter: &mut i32,
) -> Result<(), LemmyError> {
  match DeletableObjects::read_from_db(object, context).await? {
    DeletableObjects::Community(community) => {
      if community.local {
        let mod_ = actor.dereference(context, request_counter).await?;
        let object = community.actor_id();
        send_apub_delete(&mod_, &community.clone(), object, true, context).await?;
      }

      let community = blocking(context.pool(), move |conn| {
        Community::update_deleted(conn, community.id, deleted)
      })
      .await??;
      send_community_ws_message(community.id, ws_messages.community, None, None, context).await?;
    }
    DeletableObjects::Post(post) => {
      let deleted_post = blocking(context.pool(), move |conn| {
        Post::update_deleted(conn, post.id, deleted)
      })
      .await??;
      send_post_ws_message(deleted_post.id, ws_messages.post, None, None, context).await?;
    }
    DeletableObjects::Comment(comment) => {
      let deleted_comment = blocking(context.pool(), move |conn| {
        Comment::update_deleted(conn, comment.id, deleted)
      })
      .await??;
      send_comment_ws_message_simple(deleted_comment.id, ws_messages.comment, context).await?;
    }
  }
  Ok(())
}
