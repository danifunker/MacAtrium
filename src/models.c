/*
 * models.c — see models.h. Baked from data/models.jsonl (82 unique Gestalt ids);
 * a "+N same-board" note marks an id shared by N further models, where the name is
 * representative and the floor is the most permissive of the group.
 */
#include "models.h"

static const MacModel kModels[] = {
    {   1, 0x0110, "Macintosh 512K" },
    {   3, 0x0300, "Macintosh 512Ke" },
    {   4, 0x0320, "Macintosh Plus" },
    {   5, 0x0410, "Macintosh SE" },
    {   6, 0x0410, "Macintosh II" },
    {   7, 0x0602, "Macintosh IIx" },
    {   8, 0x0603, "Macintosh IIcx" },
    {   9, 0x0603, "Macintosh SE/30" },
    {  10, 0x0604, "Macintosh Portable" },
    {  11, 0x0604, "Macintosh IIci" },
    {  13, 0x0605, "Macintosh IIfx" },
    {  17, 0x0607, "Macintosh Classic" },
    {  18, 0x0606, "Macintosh IIsi" },
    {  19, 0x0607, "Mac LC" },
    {  20, 0x0701, "Quadra 900" },
    {  21, 0x0701, "PowerBook 170" },
    {  22, 0x0701, "Quadra 700" },
    {  23, 0x0608, "Macintosh Classic II" },      /* +1 same-board */
    {  24, 0x0608, "PowerBook 100" },
    {  25, 0x0701, "PowerBook 140" },
    {  26, 0x0701, "Quadra 950" },
    {  27, 0x0710, "Mac LC III" },                /* +1 same-board */
    {  29, 0x0710, "PowerBook Duo 210" },
    {  30, 0x0710, "Centris 650" },
    {  32, 0x0710, "PowerBook Duo 230" },
    {  33, 0x0710, "PowerBook 180" },
    {  34, 0x0710, "PowerBook 160" },
    {  35, 0x0710, "Quadra 800" },
    {  36, 0x0710, "Quadra 650" },
    {  37, 0x0607, "Mac LC II" },                 /* +1 same-board */
    {  38, 0x0710, "PowerBook Duo 250" },
    {  39, 0x0712, "AWS 9150" },
    {  41, 0x0751, "Performa 5200" },             /* +2 same-board */
    {  42, 0x0751, "Performa 6200" },             /* +3 same-board */
    {  44, 0x0710, "Macintosh IIvi" },
    {  45, 0x0710, "Performa 600" },
    {  48, 0x0710, "Macintosh IIvx" },
    {  49, 0x0710, "Color Classic" },             /* +1 same-board */
    {  50, 0x0710, "PowerBook 165c" },
    {  52, 0x0710, "Centris 610" },
    {  53, 0x0710, "Quadra 610" },
    {  54, 0x0701, "PowerBook 145" },             /* +1 same-board */
    {  56, 0x0710, "Mac LC 520" },                /* +1 same-board */
    {  58, 0x0751, "Performa 6360" },             /* +3 same-board */
    {  60, 0x0710, "Centris 660AV" },             /* +1 same-board */
    {  62, 0x0710, "Mac LC III+" },               /* +1 same-board */
    {  65, 0x0712, "Power Mac 8100" },
    {  67, 0x0752, "Power Mac 9500" },            /* +3 same-board */
    {  68, 0x0752, "Power Mac 7500" },            /* +1 same-board */
    {  69, 0x0752, "Power Mac 8500" },            /* +1 same-board */
    {  71, 0x0710, "PowerBook 180c" },
    {  72, 0x0711, "PowerBook 520" },             /* +4 same-board */
    {  75, 0x0712, "Power Mac 6100" },
    {  77, 0x0710, "PowerBook Duo 270c" },
    {  78, 0x0710, "Quadra 840AV" },
    {  80, 0x0710, "Mac LC 550" },                /* +1 same-board */
    {  83, 0x0710, "Color Classic II" },          /* +1 same-board */
    {  84, 0x0710, "PowerBook 165" },
    {  85, 0x0711, "PowerBook 190" },             /* +1 same-board */
    {  89, 0x0710, "Performa 475/476" },
    {  92, 0x0710, "Mac LC 575" },                /* +1 same-board */
    {  94, 0x0710, "Quadra 605" },
    {  98, 0x0710, "LC 630 DOS Compatible" },     /* +3 same-board */
    {  99, 0x0710, "Mac LC 580" },                /* +1 same-board */
    { 102, 0x0710, "PowerBook Duo 280" },
    { 103, 0x0710, "PowerBook Duo 280c" },
    { 108, 0x0752, "Power Mac 7200" },            /* +5 same-board */
    { 109, 0x0755, "Power Mac 7300" },
    { 112, 0x0712, "Power Mac 7100" },
    { 115, 0x0711, "PowerBook 150" },
    { 124, 0x0752, "PowerBook Duo 2300c" },
    { 128, 0x0752, "PowerBook 5300" },
    { 306, 0x0761, "PowerBook 3400c" },
    { 307, 0x0760, "PowerBook 2400c" },
    { 310, 0x0753, "PowerBook 1400" },
    { 312, 0x0800, "PowerBook G3 (WallStreet)" },
    { 313, 0x0800, "PowerBook G3 (Kanga)" },
    { 314, 0x0800, "PowerBook G3 Series II" },
    { 510, 0x0800, "Power Mac G3 (Beige)" },      /* +1 same-board */
    { 512, 0x0755, "Power Mac 5500" },
    { 513, 0x0753, "Power Mac 6500" },
    { 514, 0x0753, "Power Mac 4400" },            /* +1 same-board */
};

#define NMODELS ((int)(sizeof kModels / sizeof kModels[0]))

const MacModel *model_by_id(long machineID)
{
    int i;
    if (machineID <= 0) return 0;
    for (i = 0; i < NMODELS; i++)
        if (kModels[i].id == machineID) return &kModels[i];
    return 0;
}
