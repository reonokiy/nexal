from nexal.channels.channel import Channel, ImageAttachment, IncomingMessage, OnMessage


def chunk_message(text: str, max_len: int) -> list[str]:
    """Split a long message into chunks, breaking at paragraph boundaries."""
    if len(text) <= max_len:
        return [text]

    chunks: list[str] = []
    remaining = text
    while remaining:
        if len(remaining) <= max_len:
            chunks.append(remaining)
            break
        # Try to split at a double newline (paragraph break).
        cut = remaining.rfind("\n\n", 0, max_len)
        if cut == -1:
            # Fall back to single newline.
            cut = remaining.rfind("\n", 0, max_len)
        if cut == -1:
            # Fall back to space.
            cut = remaining.rfind(" ", 0, max_len)
        if cut == -1:
            # Hard cut.
            cut = max_len
        chunks.append(remaining[:cut])
        remaining = remaining[cut:].lstrip("\n")
    return chunks


__all__ = ["Channel", "ImageAttachment", "IncomingMessage", "OnMessage", "chunk_message"]
