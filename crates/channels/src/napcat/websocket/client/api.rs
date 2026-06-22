//! NapCatQQ WebSocket 客户端的 OneBot API 调用封装。
//!
//! 这些方法都是 `call_api` 的薄封装：构造 `ApiRequest`、调用 `call_api`、
//! 解析 `ApiResponse`。从 `client.rs` 分离出来以缩小主文件、按职责聚合 API。

use blockcell_core::{Error, Result};

use super::super::super::types::ApiRequest;
use super::NapCatWsClient;

impl NapCatWsClient {
    /// Send a private message via WebSocket.
    pub async fn send_private_msg(
        &self,
        user_id: &str,
        message: &serde_json::Value,
    ) -> Result<i64> {
        let request = ApiRequest::send_private_msg(user_id, message, None, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "send_private_msg failed: {}",
                response.error_message()
            )));
        }
        let msg_id = response
            .data
            .get("message_id")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        Ok(msg_id)
    }

    /// Send a group message via WebSocket.
    pub async fn send_group_msg(&self, group_id: &str, message: &serde_json::Value) -> Result<i64> {
        let request = ApiRequest::send_group_msg(group_id, message, None, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "send_group_msg failed: {}",
                response.error_message()
            )));
        }
        let msg_id = response
            .data
            .get("message_id")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        Ok(msg_id)
    }

    /// Recall a message via WebSocket.
    pub async fn delete_msg(&self, message_id: i64) -> Result<()> {
        let request = ApiRequest::delete_msg(message_id, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "delete_msg failed: {}",
                response.error_message()
            )));
        }
        Ok(())
    }

    /// Get a message via WebSocket.
    pub async fn get_msg(&self, message_id: i64) -> Result<serde_json::Value> {
        let request = ApiRequest::get_msg(message_id, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "get_msg failed: {}",
                response.error_message()
            )));
        }
        Ok(response.data)
    }

    /// Get login info via WebSocket.
    pub async fn get_login_info(&self) -> Result<serde_json::Value> {
        let request = ApiRequest::get_login_info(None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "get_login_info failed: {}",
                response.error_message()
            )));
        }
        Ok(response.data)
    }

    /// Get group list via WebSocket.
    pub async fn get_group_list(&self) -> Result<Vec<serde_json::Value>> {
        let request = ApiRequest::get_group_list(None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "get_group_list failed: {}",
                response.error_message()
            )));
        }
        let groups: Vec<serde_json::Value> = serde_json::from_value(response.data)
            .map_err(|e| Error::Channel(format!("Failed to parse group list: {}", e)))?;
        Ok(groups)
    }

    /// Get friend list via WebSocket.
    pub async fn get_friend_list(&self) -> Result<Vec<serde_json::Value>> {
        let request = ApiRequest::get_friend_list(None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "get_friend_list failed: {}",
                response.error_message()
            )));
        }
        let friends: Vec<serde_json::Value> = serde_json::from_value(response.data)
            .map_err(|e| Error::Channel(format!("Failed to parse friend list: {}", e)))?;
        Ok(friends)
    }

    // =========================================================================
    // Group Management via WebSocket
    // =========================================================================

    /// Set group admin via WebSocket.
    pub async fn set_group_admin(&self, group_id: &str, user_id: &str, enable: bool) -> Result<()> {
        let request = ApiRequest::set_group_admin(group_id, user_id, enable, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "set_group_admin failed: {}",
                response.error_message()
            )));
        }
        Ok(())
    }

    /// Set group card via WebSocket.
    pub async fn set_group_card(&self, group_id: &str, user_id: &str, card: &str) -> Result<()> {
        let request = ApiRequest::set_group_card(group_id, user_id, card, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "set_group_card failed: {}",
                response.error_message()
            )));
        }
        Ok(())
    }

    /// Set group name via WebSocket.
    pub async fn set_group_name(&self, group_id: &str, group_name: &str) -> Result<()> {
        let request = ApiRequest::set_group_name(group_id, group_name, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "set_group_name failed: {}",
                response.error_message()
            )));
        }
        Ok(())
    }

    /// Get group member info via WebSocket.
    pub async fn get_group_member_info(
        &self,
        group_id: &str,
        user_id: &str,
        no_cache: bool,
    ) -> Result<serde_json::Value> {
        let request = ApiRequest::get_group_member_info(group_id, user_id, no_cache, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "get_group_member_info failed: {}",
                response.error_message()
            )));
        }
        Ok(response.data)
    }

    /// Get group member list via WebSocket.
    pub async fn get_group_member_list(&self, group_id: &str) -> Result<Vec<serde_json::Value>> {
        let request = ApiRequest::get_group_member_list(group_id, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "get_group_member_list failed: {}",
                response.error_message()
            )));
        }
        let members: Vec<serde_json::Value> = serde_json::from_value(response.data)
            .map_err(|e| Error::Channel(format!("Failed to parse member list: {}", e)))?;
        Ok(members)
    }

    /// Set group kick via WebSocket.
    pub async fn set_group_kick(&self, group_id: &str, user_id: &str) -> Result<()> {
        let request = ApiRequest::set_group_kick(group_id, user_id, None, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "set_group_kick failed: {}",
                response.error_message()
            )));
        }
        Ok(())
    }

    /// Set group ban via WebSocket.
    pub async fn set_group_ban(&self, group_id: &str, user_id: &str, duration: u32) -> Result<()> {
        let request = ApiRequest::set_group_ban(group_id, user_id, duration, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "set_group_ban failed: {}",
                response.error_message()
            )));
        }
        Ok(())
    }

    /// Set group whole ban via WebSocket.
    pub async fn set_group_whole_ban(&self, group_id: &str, enable: bool) -> Result<()> {
        let request = ApiRequest::set_group_whole_ban(group_id, enable, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "set_group_whole_ban failed: {}",
                response.error_message()
            )));
        }
        Ok(())
    }

    /// Leave a group via WebSocket.
    pub async fn set_group_leave(&self, group_id: &str, is_dismiss: bool) -> Result<()> {
        let request = ApiRequest::set_group_leave(group_id, is_dismiss, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "set_group_leave failed: {}",
                response.error_message()
            )));
        }
        Ok(())
    }

    /// Set group special title via WebSocket.
    pub async fn set_group_special_title(
        &self,
        group_id: &str,
        user_id: &str,
        special_title: &str,
    ) -> Result<()> {
        let request = ApiRequest::set_group_special_title(group_id, user_id, special_title, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "set_group_special_title failed: {}",
                response.error_message()
            )));
        }
        Ok(())
    }

    // =========================================================================
    // User Info via WebSocket
    // =========================================================================

    /// Get stranger info via WebSocket.
    pub async fn get_stranger_info(
        &self,
        user_id: &str,
        no_cache: bool,
    ) -> Result<serde_json::Value> {
        let request = ApiRequest::get_stranger_info(user_id, no_cache, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "get_stranger_info failed: {}",
                response.error_message()
            )));
        }
        Ok(response.data)
    }

    /// Send like via WebSocket.
    pub async fn send_like(&self, user_id: &str, times: u32) -> Result<()> {
        let request = ApiRequest::send_like(user_id, times, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "send_like failed: {}",
                response.error_message()
            )));
        }
        Ok(())
    }

    /// Set friend remark via WebSocket.
    pub async fn set_friend_remark(&self, user_id: &str, remark: &str) -> Result<()> {
        let request = ApiRequest::set_friend_remark(user_id, remark, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "set_friend_remark failed: {}",
                response.error_message()
            )));
        }
        Ok(())
    }

    /// Delete friend via WebSocket.
    pub async fn delete_friend(&self, user_id: &str) -> Result<()> {
        let request = ApiRequest::delete_friend(user_id, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "delete_friend failed: {}",
                response.error_message()
            )));
        }
        Ok(())
    }

    // =========================================================================
    // File Operations via WebSocket
    // =========================================================================

    /// Upload group file via WebSocket.
    pub async fn upload_group_file(
        &self,
        group_id: &str,
        file: &str,
        name: Option<&str>,
    ) -> Result<serde_json::Value> {
        let request = ApiRequest::upload_group_file(group_id, file, name, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "upload_group_file failed: {}",
                response.error_message()
            )));
        }
        Ok(response.data)
    }

    /// Get group file system info via WebSocket.
    pub async fn get_group_file_system_info(&self, group_id: &str) -> Result<serde_json::Value> {
        let request = ApiRequest::get_group_file_system_info(group_id, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "get_group_file_system_info failed: {}",
                response.error_message()
            )));
        }
        Ok(response.data)
    }

    /// Get group files by folder via WebSocket.
    pub async fn get_group_files_by_folder(
        &self,
        group_id: &str,
        folder_id: &str,
    ) -> Result<serde_json::Value> {
        let request = ApiRequest::get_group_files_by_folder(group_id, folder_id, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "get_group_files_by_folder failed: {}",
                response.error_message()
            )));
        }
        Ok(response.data)
    }

    /// Delete group file via WebSocket.
    pub async fn delete_group_file(
        &self,
        group_id: &str,
        file_id: &str,
        busid: Option<i32>,
    ) -> Result<()> {
        let request = ApiRequest::delete_file(group_id, file_id, busid, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "delete_group_file failed: {}",
                response.error_message()
            )));
        }
        Ok(())
    }

    // =========================================================================
    // Misc Operations via WebSocket
    // =========================================================================

    /// Get status via WebSocket.
    pub async fn get_status(&self) -> Result<serde_json::Value> {
        let request = ApiRequest::get_status(None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "get_status failed: {}",
                response.error_message()
            )));
        }
        Ok(response.data)
    }

    /// Get version info via WebSocket.
    pub async fn get_version_info(&self) -> Result<serde_json::Value> {
        let request = ApiRequest::get_version_info(None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "get_version_info failed: {}",
                response.error_message()
            )));
        }
        Ok(response.data)
    }

    /// Set QQ profile via WebSocket.
    pub async fn set_qq_profile(
        &self,
        nickname: Option<&str>,
        personal_note: Option<&str>,
        sex: Option<&str>,
    ) -> Result<()> {
        let request = ApiRequest::set_qq_profile(nickname, personal_note, sex, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "set_qq_profile failed: {}",
                response.error_message()
            )));
        }
        Ok(())
    }

    /// Get cookies via WebSocket.
    pub async fn get_cookies(&self, domain: &str) -> Result<serde_json::Value> {
        let request = ApiRequest::get_cookies(domain, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "get_cookies failed: {}",
                response.error_message()
            )));
        }
        Ok(response.data)
    }

    /// Get CSRF token via WebSocket.
    pub async fn get_csrf_token(&self) -> Result<serde_json::Value> {
        let request = ApiRequest::get_csrf_token(None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "get_csrf_token failed: {}",
                response.error_message()
            )));
        }
        Ok(response.data)
    }

    // =========================================================================
    // Extended Message APIs via WebSocket
    // =========================================================================

    /// Get forward message content via WebSocket.
    pub async fn get_forward_msg(&self, message_id: &str) -> Result<serde_json::Value> {
        let request = ApiRequest::get_forward_msg(message_id, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "get_forward_msg failed: {}",
                response.error_message()
            )));
        }
        Ok(response.data)
    }

    /// Set message emoji like via WebSocket.
    pub async fn set_msg_emoji_like(&self, message_id: i64, emoji_id: &str) -> Result<()> {
        let request = ApiRequest::set_msg_emoji_like(message_id, emoji_id, None, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "set_msg_emoji_like failed: {}",
                response.error_message()
            )));
        }
        Ok(())
    }

    /// Mark message as read via WebSocket.
    pub async fn mark_msg_as_read(&self, message_id: i64) -> Result<()> {
        let request = ApiRequest::mark_msg_as_read(message_id, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "mark_msg_as_read failed: {}",
                response.error_message()
            )));
        }
        Ok(())
    }

    // =========================================================================
    // Essence Message APIs via WebSocket
    // =========================================================================

    /// Set essence message via WebSocket.
    pub async fn set_essence_msg(&self, message_id: i64) -> Result<()> {
        let request = ApiRequest::set_essence_msg(message_id, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "set_essence_msg failed: {}",
                response.error_message()
            )));
        }
        Ok(())
    }

    /// Delete essence message via WebSocket.
    pub async fn delete_essence_msg(&self, message_id: i64) -> Result<()> {
        let request = ApiRequest::delete_essence_msg(message_id, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "delete_essence_msg failed: {}",
                response.error_message()
            )));
        }
        Ok(())
    }

    /// Get essence message list via WebSocket.
    pub async fn get_essence_msg_list(&self, group_id: &str) -> Result<Vec<serde_json::Value>> {
        let request = ApiRequest::get_essence_msg_list(group_id, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "get_essence_msg_list failed: {}",
                response.error_message()
            )));
        }
        let list: Vec<serde_json::Value> = serde_json::from_value(response.data)
            .map_err(|e| Error::Channel(format!("Failed to parse essence list: {}", e)))?;
        Ok(list)
    }

    // =========================================================================
    // Group Extended APIs via WebSocket
    // =========================================================================

    /// Get group at all remain count via WebSocket.
    pub async fn get_group_at_all_remain(&self, group_id: &str) -> Result<serde_json::Value> {
        let request = ApiRequest::get_group_at_all_remain(group_id, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "get_group_at_all_remain failed: {}",
                response.error_message()
            )));
        }
        Ok(response.data)
    }

    /// Set group portrait via WebSocket.
    pub async fn set_group_portrait(&self, group_id: &str, file: &str) -> Result<()> {
        let request = ApiRequest::set_group_portrait(group_id, file, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "set_group_portrait failed: {}",
                response.error_message()
            )));
        }
        Ok(())
    }

    // =========================================================================
    // Media/Resource APIs via WebSocket
    // =========================================================================

    /// Get image info via WebSocket.
    pub async fn get_image(&self, file: &str) -> Result<serde_json::Value> {
        let request = ApiRequest::get_image(Some(file), None, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "get_image failed: {}",
                response.error_message()
            )));
        }
        Ok(response.data)
    }

    /// Get record (voice) info via WebSocket.
    pub async fn get_record(&self, file: &str, out_format: &str) -> Result<serde_json::Value> {
        let request = ApiRequest::get_record(Some(file), None, out_format, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "get_record failed: {}",
                response.error_message()
            )));
        }
        Ok(response.data)
    }

    /// Download file via WebSocket.
    pub async fn download_file(
        &self,
        url: &str,
        thread_count: Option<i32>,
        headers: Option<&[&str]>,
    ) -> Result<serde_json::Value> {
        let request = ApiRequest::download_file(url, thread_count, headers, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "download_file failed: {}",
                response.error_message()
            )));
        }
        Ok(response.data)
    }

    // =========================================================================
    // Capability Check APIs via WebSocket
    // =========================================================================

    /// Check if can send image via WebSocket.
    pub async fn can_send_image(&self) -> Result<bool> {
        let request = ApiRequest::can_send_image(None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "can_send_image failed: {}",
                response.error_message()
            )));
        }
        let yes = response
            .data
            .get("yes")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        Ok(yes)
    }

    /// Check if can send record via WebSocket.
    pub async fn can_send_record(&self) -> Result<bool> {
        let request = ApiRequest::can_send_record(None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "can_send_record failed: {}",
                response.error_message()
            )));
        }
        let yes = response
            .data
            .get("yes")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        Ok(yes)
    }

    // =========================================================================
    // Friend/Group Request Handling via WebSocket
    // =========================================================================

    /// Handle friend request via WebSocket.
    pub async fn set_friend_add_request(&self, flag: &str, approve: bool) -> Result<()> {
        let request = ApiRequest::set_friend_add_request(flag, approve, None, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "set_friend_add_request failed: {}",
                response.error_message()
            )));
        }
        Ok(())
    }

    /// Handle group request via WebSocket.
    pub async fn set_group_add_request(
        &self,
        flag: &str,
        sub_type: &str,
        approve: bool,
    ) -> Result<()> {
        let request = ApiRequest::set_group_add_request(flag, sub_type, approve, None, None);
        let response = self.call_api(request).await?;
        if !response.is_success() {
            return Err(Error::Channel(format!(
                "set_group_add_request failed: {}",
                response.error_message()
            )));
        }
        Ok(())
    }
}
