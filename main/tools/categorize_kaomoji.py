"""Prefix each kaomoji keyword with its main category.

Reads the untouched original and rewrites the file the app uses:
    python3 tools/categorize_kaomoji.py ../kaomojis-msime.orig.txt ../kaomojis-msime.txt
(run from the test/ folder), then rebuild with: cargo build --release

To change categories: edit CATS below. A category's synonyms decide which
keywords get that category's name put in front of them. If you add, remove or
rename a category, make the same change to KAO_CATS in build.rs (same order).
"""
import re, sys
from collections import Counter

CATS = {
    "happy": ["confident", "determined", "cozy", "relieved", "gentle", "clap", "clapping", "proud kaomoji", "smile", "smiling", "overjoyed", "beaming", "joy", "joyful", "laughing", "laugh",
              "cheerful", "excited", "party", "celebration", "celebrate", "dancing", "dance",
              "grin", "glad", "delighted", "yay", "victory", "proud", "satisfied", "blissful",
              "relaxed", "content", "fun", "energetic", "motivated", "good luck", "cheer", "cheering", "smug", "playful", "silly", "cool", "calm", "finger guns", "friends", "friendship", "lenny face", "peace", "thumbs up"],
    "cute": ["peek", "rainbow", "mischievous", "oshi", "coquette", "girly", "wink", "adorable", "kawaii", "pretty", "fluffy", "sparkle",
             "sparkly", "innocent", "dreamy", "magic", "magical", "idol", "princess", "angel", "flower", "flowers", "star", "star kaomoji", "peeking", "sneaky", "ribbon", "bow tie"],
    "love": ["flirty", "couple", "flirt", "date", "heart", "hearts", "kiss", "affection", "hug", "hugging", "blushing", "blush", "shy",
             "embarrassed", "crush", "romantic", "romance", "valentine", "in love", "lovey"],
    "crying": ["sobbing", "bawling", "teary", "tears", "tearful", "weeping", "cry", "cries", "sob"],
    "sad": ["dejected", "lonely", "melancholy", "depressed", "sorry", "apology", "apologize",
            "disappointed", "gloomy", "heartbroken", "stressed", "bored", "boredom", "hurt", "despair", "regret", "sulking",
            "pout", "pouting", "sulk"],
    "angry": ["jealous", "facepalm", "disapproval", "sarcastic", "dramatic", "annoyed", "mad", "rage", "furious", "table flip", "irritated", "grumpy", "upset",
              "frustrated", "fight", "fighting", "punch", "hmph", "angry kaomoji", "devil", "evil", "villain"],
    "surprised": ["shocked", "shock", "confused", "scared", "nervous", "worried", "astonished",
                  "amazed", "panic", "panicking", "afraid", "fear", "frightened", "startled",
                  "awkward", "troubled", "anxious", "huh", "question", "thinking", "shrug", "wow"],
    "sleepy": ["goodnight", "good-night", "tired", "yawn", "yawning", "good night", "sleep", "sleeping", "exhausted", "drowsy",
               "nap", "zzz", "rest", "lazy", "dizzy", "sick", "faint"],
    "greeting": ["wave", "waving", "hello", "hi", "good morning", "thank you", "thanks", "bye",
                 "goodbye", "see you", "welcome", "congratulations", "congrats", "good evening",
                 "nice to meet you", "please", "ok", "okay", "salute", "bow"],
    "animal": ["hedgehog", "raccoon", "elephant", "giraffe", "alpaca", "dinosaur", "dragon", "unicorn", "shark", "hamster kaomoji", "nature kaomoji", "cat", "dog", "bear", "rabbit", "bunny", "fox", "kitty", "kitten", "puppy", "pig",
               "bird", "mouse", "hamster", "penguin", "panda", "frog", "fish", "monkey", "sheep",
               "cow", "horse", "owl", "chick", "duck", "lion", "tiger", "wolf", "squirrel",
               "turtle", "snake", "bat", "spider", "crab", "octopus", "whale", "dolphin", "seal",
               "otter", "koala", "deer", "animal kaomoji", "neko", "inu", "kuma", "usagi"],
    "food": ["drooling", "thirsty", "pepero day", "coffee", "tea", "food kaomoji", "eating", "eat", "hungry", "drink", "drinking",
             "cake", "beer", "sweets", "candy", "ramen", "sushi", "bread", "rice", "dessert",
             "ice cream", "snack", "cooking", "meal", "juice", "milk", "wine", "lunch", "dinner",
             "breakfast", "chocolate", "delicious", "yummy"],
    "seasonal": ["fathers day", "carnival", "festival", "pepero", "summer", "easter", "birthday", "golden week", "april fools", "weather", "winter",
                 "spring", "autumn", "fall", "christmas", "halloween", "new year", "rain", "rainy",
                 "snow", "sunny", "hot", "cold", "holiday", "vacation", "beach", "tanabata",
                 "valentine's day", "hanami", "cherry blossom", "fireworks", "mother's day", "mothers day", "father's day", "graduation", "sakura", "hinamatsuri", "children's day", "white day", "obon", "tsukimi", "setsubun"],
    "activity": ["jumping", "stretching", "meditation", "praying", "jazz", "ninja", "action kaomoji", "dance kaomoji", "music", "reading", "running", "gaming", "singing", "sports", "sports kaomoji", "work",
                 "work kaomoji", "working", "yoga", "travel", "selfie", "study", "studying", "writing",
                 "drawing", "shopping", "swimming", "exercise", "workout", "dancing kaomoji", "game",
                 "games", "phone", "computer", "typing", "camera", "photo", "guitar", "piano", "soccer",
                 "baseball", "basketball", "tennis", "walking", "driving", "cleaning", "school",
                 "pointing", "listening", "karaoke", "fishing", "camping", "bath", "gym", "office"],
    "decorated": ["text art", "jirai kei", "ryousangata", "stylish", "decorated kaomoji", "decoration",
                  "fancy", "ornate", "aa", "ascii art", "border", "frame"],
}
CAT_NAMES = list(CATS)

def has_phrase(text, phrase):
    return re.search(r"(?<![a-z])" + re.escape(phrase) + r"s?(?![a-z])", text) is not None

def classify(kw):
    k = kw.lower()
    for c in CAT_NAMES:
        if has_phrase(k, c):
            return c, False  # already contains the category word
    for c in CAT_NAMES:
        for syn in CATS[c]:
            if has_phrase(k, syn):
                return c, True
    return None, False

src, dst = sys.argv[1], sys.argv[2]
out, gained, other, total = [], Counter(), Counter(), Counter()
with open(src, encoding="utf-8") as f:
    for line in f:
        line = line.rstrip("\n")
        if not line or line.startswith("!"):
            continue
        parts = line.split("\t")
        kw, kao = parts[0].strip(), parts[1]
        cat, add = classify(kw)
        if cat is None:
            other[kw.lower()] += 1
        else:
            total[cat] += 1
            if add:
                gained[cat] += 1
                kw = f"{cat} {kw}"
        out.append(f"{kw.lower()}\t{kao}")

with open(dst, "w", encoding="utf-8") as f:
    f.write("\n".join(out) + "\n")

print(f"lines: {len(out)}")
for c in CAT_NAMES:
    print(f"  {c:10} total {total[c]:6}  prefixed {gained[c]:6}")
print(f"  other      {sum(other.values()):6}  ({len(other)} distinct keywords)")
print("top unmatched:", ", ".join(f"{k}({n})" for k, n in other.most_common(40)))
