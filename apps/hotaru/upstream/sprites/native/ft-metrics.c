/* Diagnostic only: pinned Ghostty Linux uses FT y_ppem and raw post values. */
#include <ft2build.h>
#include FT_FREETYPE_H
#include FT_TRUETYPE_TABLES_H
#include <stdio.h>
#include <stdlib.h>
#include <math.h>
int main(int argc,char **argv) {
    if(argc!=4)return 2;
    FT_Library library;FT_Face face;
    if(FT_Init_FreeType(&library))return 3;
    if(FT_New_Face(library,argv[1],0,&face))return 4;
    const float points=(float)strtod(argv[2],NULL);
    const double scale=strtod(argv[3],NULL);
    const unsigned dpi=(unsigned)lround(96.0*scale);
    if(FT_Set_Char_Size(face,0,(FT_F26Dot6)lroundf(points*64),dpi,dpi))return 5;
    const TT_Postscript *post=FT_Get_Sfnt_Table(face,ft_sfnt_post);
    if(!post || post->underlineThickness<=0)return 6; /* fallback audited separately */
    double thick=(double)post->underlineThickness*face->size->metrics.y_ppem/face->units_per_EM;
    printf("{\"ppem\":%u,\"post_thickness\":%d,\"upm\":%u,\"box_thickness\":%.0f}\n",
        face->size->metrics.y_ppem,post->underlineThickness,face->units_per_EM,fmax(1,ceil(thick)));
    FT_Done_Face(face);FT_Done_FreeType(library);return 0;
}
