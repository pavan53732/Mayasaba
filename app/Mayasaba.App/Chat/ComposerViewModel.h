// Mayasaba Control Room — plain C++ composer view-model.
//
// Owns only UI-local presentation state: the unsent draft, pending attachment selections, and
// the keyboard send preference. It never writes requirements, decisions, ProjectIntent or
// project epochs, and it never submits on its own — the page calls ChatViewModel, which calls
// the controller's SendMessage. One composer, one contribution path, for the first and every
// later message (AGENTS.md section 5).
#pragma once

#include <cstdint>
#include <string>
#include <vector>

#include "ControllerBridge.h"

namespace mayasaba::app {

// One pending attachment in the composer.
struct AttachmentViewModel {
    std::string name;
    std::string path;       // absolute path chosen by the user
    std::uint64_t size = 0;
    std::string sha256;
    std::string state_text; // "Selected" — nothing is claimed until the controller persists it
};

// "1.2 KB", "3.4 MB". Presentation only.
std::string FormatByteSize(std::uint64_t bytes);

class ComposerViewModel {
public:
    const std::string& Draft() const { return draft_; }
    void SetDraft(std::string text) { draft_ = std::move(text); }

    // Send is disabled ONLY until an authorized project root is bound. Draft emptiness is a
    // convenience guard, not an authority rule.
    bool CanSend() const { return bound_ && !draft_.empty(); }
    bool Bound() const { return bound_; }
    void SetBound(bool bound) { bound_ = bound; }

    bool EnterSends() const { return enter_sends_; }
    void SetEnterSends(bool value) { enter_sends_ = value; }

    const std::vector<AttachmentViewModel>& Attachments() const { return attachments_; }
    void AddAttachment(AttachmentViewModel attachment) { attachments_.push_back(std::move(attachment)); }
    void RemoveAttachmentAt(std::size_t index) {
        if (index < attachments_.size()) {
            attachments_.erase(attachments_.begin() + static_cast<std::ptrdiff_t>(index));
        }
    }
    void ClearAttachments() { attachments_.clear(); }

    // A rejected submission preserves the draft and valid attachments (UI-UX.md section 7.2),
    // so this is called only after the controller has persisted the contribution.
    void ClearAfterPersisted() {
        draft_.clear();
        attachments_.clear();
    }

    // The typed attachment references the controller expects on SendMessage.
    std::vector<mayasaba::control::AttachmentRef> ToAttachmentRefs() const;

private:
    std::string draft_;
    bool bound_ = false;
    bool enter_sends_ = true;
    std::vector<AttachmentViewModel> attachments_;
};

}  // namespace mayasaba::app
