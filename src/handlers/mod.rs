use std::sync::Arc;
use std::sync::Mutex;

use tbot::{
    connectors::Https,
    contexts::{Command, Text},
    types::{input_file, parameters},
};

use crate::client::pull_feed;
use crate::data::Database;
use crate::messages::{format_large_msg, Escape};

mod opml;

#[derive(Debug, Copy, Clone)]
struct MsgTarget {
    chat_id: tbot::types::chat::Id,
    message_id: tbot::types::message::Id,
    first_time: bool,
}

impl MsgTarget {
    fn new(chat_id: tbot::types::chat::Id, message_id: tbot::types::message::Id) -> Self {
        MsgTarget {
            chat_id,
            message_id,
            first_time: true,
        }
    }
    fn update(self, message_id: tbot::types::message::Id) -> Self {
        MsgTarget {
            message_id,
            first_time: false,
            ..self
        }
    }
}

pub async fn rss(db: Arc<Mutex<Database>>, cmd: Arc<Command<Text<Https>>>) -> anyhow::Result<()> {
    let chat_id = cmd.chat.id;
    let channel = &cmd.text.value;
    let mut target_id = chat_id;

    if !channel.is_empty() {
        let user_id = cmd.from.as_ref().unwrap().id;
        let channel_id = check_channel_permission(
            &cmd.bot,
            channel,
            MsgTarget::new(chat_id, cmd.message_id),
            user_id,
        )
        .await?;
        if channel_id.is_none() {
            return Ok(());
        }
        target_id = channel_id.unwrap();
    }

    let feeds = db.lock().unwrap().subscribed_feeds(target_id.0);
    let msgs = if let Some(feeds) = feeds {
        format_large_msg("订阅列表：".to_string(), &feeds, |feed| {
            format!(
                "<a href=\"{}\">{}</a>",
                Escape(&feed.link),
                Escape(&feed.title)
            )
        })
    } else {
        vec!["订阅列表为空".to_string()]
    };

    let mut prev_msg = cmd.message_id;
    for msg in msgs {
        let text = parameters::Text::html(&msg);
        let msg = cmd
            .bot
            .send_message(chat_id, text)
            .reply_to_message_id(prev_msg)
            .call()
            .await?;
        prev_msg = msg.id;
    }
    Ok(())
}

pub async fn sub(db: Arc<Mutex<Database>>, cmd: Arc<Command<Text<Https>>>) -> anyhow::Result<()> {
    let chat_id = cmd.chat.id;
    let text = &cmd.text.value;
    let args = text.split_whitespace().collect::<Vec<_>>();
    let mut target_id = chat_id;
    let feed_url;

    match &*args {
        [url] => feed_url = url,
        [channel, url] => {
            let user_id = cmd.from.as_ref().unwrap().id;
            let channel_id = check_channel_permission(
                &cmd.bot,
                channel,
                MsgTarget::new(chat_id, cmd.message_id),
                user_id,
            )
            .await?;
            if channel_id.is_none() {
                return Ok(());
            }
            target_id = channel_id.unwrap();
            feed_url = url;
        }
        [..] => {
            return Ok(());
        }
    };
    if db.lock().unwrap().is_subscribed(target_id.0, feed_url) {
        update_response(
            &cmd.bot,
            MsgTarget::new(chat_id, cmd.message_id),
            parameters::Text::html("已订阅过的 RSS"),
        )
        .await?;
        return Ok(());
    }
    let resp_ctx = update_response(
        &cmd.bot,
        MsgTarget::new(chat_id, cmd.message_id),
        parameters::Text::plain("处理中，请稍候"),
    )
    .await?;
    let msg = match pull_feed(feed_url).await {
        Ok(feed) => {
            if db.lock().unwrap().subscribe(target_id.0, feed_url, &feed) {
                format!(
                    "《<a href=\"{}\">{}</a>》 订阅成功",
                    Escape(&feed.link),
                    Escape(&feed.title)
                )
            } else {
                "已订阅过的 RSS".into()
            }
        }
        Err(e) => format!("订阅失败：{}", Escape(&e.to_string())),
    };
    update_response(&cmd.bot, resp_ctx, parameters::Text::html(&msg)).await?;
    Ok(())
}

pub async fn unsub(db: Arc<Mutex<Database>>, cmd: Arc<Command<Text<Https>>>) -> anyhow::Result<()> {
    let chat_id = cmd.chat.id;
    let text = &cmd.text.value;
    let args = text.split_whitespace().collect::<Vec<_>>();
    let mut target_id = chat_id;
    let feed_url;

    match &*args {
        [url] => feed_url = url,
        [channel, url] => {
            let user_id = cmd.from.as_ref().unwrap().id;
            let channel_id = check_channel_permission(
                &cmd.bot,
                channel,
                MsgTarget::new(chat_id, cmd.message_id),
                user_id,
            )
            .await?;
            if channel_id.is_none() {
                return Ok(());
            }
            target_id = channel_id.unwrap();
            feed_url = url;
        }
        [..] => {
            return Ok(());
        }
    };
    let msg = if let Some(feed) = db.lock().unwrap().unsubscribe(target_id.0, feed_url) {
        format!(
            "《<a href=\"{}\">{}</a>》 退订成功",
            Escape(&feed.link),
            Escape(&feed.title)
        )
    } else {
        "未订阅过的 RSS".into()
    };
    update_response(
        &cmd.bot,
        MsgTarget::new(chat_id, cmd.message_id),
        parameters::Text::html(&msg),
    )
    .await?;
    Ok(())
}

pub async fn export(
    db: Arc<Mutex<Database>>,
    cmd: Arc<Command<Text<Https>>>,
) -> anyhow::Result<()> {
    let chat_id = cmd.chat.id;
    let channel = &cmd.text.value;
    let mut target_id = chat_id;

    if !channel.is_empty() {
        let user_id = cmd.from.as_ref().unwrap().id;
        let channel_id = check_channel_permission(
            &cmd.bot,
            channel,
            MsgTarget::new(chat_id, cmd.message_id),
            user_id,
        )
        .await?;
        if channel_id.is_none() {
            return Ok(());
        }
        target_id = channel_id.unwrap();
    }

    let feeds = db.lock().unwrap().subscribed_feeds(target_id.0);
    if feeds.is_none() {
        update_response(
            &cmd.bot,
            MsgTarget::new(chat_id, cmd.message_id),
            parameters::Text::plain("订阅列表为空"),
        )
        .await?;
        return Ok(());
    }
    let opml = opml::into_opml(feeds.unwrap());

    cmd.bot
        .send_document(
            chat_id,
            input_file::Document::bytes("feeds.opml", opml.as_bytes()),
        )
        .reply_to_message_id(cmd.message_id)
        .call()
        .await?;
    Ok(())
}

async fn update_response(
    bot: &tbot::Bot<Https>,
    target: MsgTarget,
    message: parameters::Text<'_>,
) -> Result<MsgTarget, tbot::errors::MethodCall> {
    let msg = if target.first_time {
        bot.send_message(target.chat_id, message)
            .reply_to_message_id(target.message_id)
            .call()
            .await?
    } else {
        bot.edit_message_text(target.chat_id, target.message_id, message)
            .call()
            .await?
    };
    Ok(target.update(msg.id))
}

async fn check_channel_permission(
    bot: &tbot::Bot<Https>,
    channel: &str,
    target: MsgTarget,
    user_id: tbot::types::user::Id,
) -> Result<Option<tbot::types::chat::Id>, tbot::errors::MethodCall> {
    let channel_id = channel
        .parse::<i64>()
        .map(|id| parameters::ChatId::Id(id.into()))
        .unwrap_or_else(|_| {
            if channel.starts_with('@') {
                parameters::ChatId::Username(&channel[1..])
            } else {
                parameters::ChatId::Username(channel)
            }
        });

    let chat = bot.get_chat(channel_id).call().await?;
    if !chat.kind.is_channel() {
        update_response(bot, target, parameters::Text::plain("目标需为 Channel")).await?;
        return Ok(None);
    }
    let admins = bot.get_chat_administrators(channel_id).call().await?;
    let user_is_admin = admins
        .iter()
        .find(|member| member.user.id == user_id)
        .is_some();
    if !user_is_admin {
        update_response(
            bot,
            target,
            parameters::Text::plain("该命令只能由 Channel 管理员使用"),
        )
        .await?;
        return Ok(None);
    }
    let bot_is_admin = admins
        .iter()
        .find(|member| member.user.id == *crate::BOT_ID.get().unwrap())
        .is_some();
    if !bot_is_admin {
        update_response(
            bot,
            target,
            parameters::Text::plain("请将本 Bot 设为管理员"),
        )
        .await?;
        return Ok(None);
    }
    Ok(Some(chat.id))
}
